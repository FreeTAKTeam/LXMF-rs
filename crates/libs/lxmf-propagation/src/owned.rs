use crate::{
    owned_changes, prepared, Error, MessageKind, PreparedMessage, Recovery, RecoveryLease, State,
    Status, StoreLimits,
};
use rns_core::identity::PrivateIdentity;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// The caller serializes access to this connection. The daemon uses the shared
/// SQL functions on its own existing writer instead of opening this service.
pub struct OwnedStore {
    pub(crate) conn: Connection,
    pub(crate) identity: PrivateIdentity,
    limits: StoreLimits,
    pub(crate) in_flight: Option<RecoveryLease>,
    stopped: bool,
}

impl OwnedStore {
    pub fn open(
        path: impl AsRef<Path>,
        identity: PrivateIdentity,
        limits: StoreLimits,
    ) -> Result<Self, Error> {
        let conn = Connection::open(path)?;
        crate::owned_schema::initialize(&conn, &identity, limits)?;
        Ok(Self { conn, identity, limits, in_flight: None, stopped: false })
    }

    pub fn deposit_own(
        &mut self,
        message: PreparedMessage,
        kind: MessageKind,
        now: i64,
        earlier_expiry: Option<i64>,
    ) -> Result<Status, Error> {
        self.ensure_running()?;
        if message.owner != prepared::delivery_address(self.identity.as_identity()) {
            return Err(Error::ForeignMessage);
        }
        let (kind, key) = kind.sql()?;
        let expires_at =
            earlier_expiry.unwrap_or(i64::MAX).min(now.saturating_add(self.limits.retention_secs));
        let message_id = hex::encode(message.message_id);
        let tx = self.conn.unchecked_transaction()?;
        owned_changes::expire(&tx, now)?;
        tx.execute(
            "DELETE FROM propagation_owned WHERE state>=4 AND projected=1 AND admitted_at<=?1",
            [now.saturating_sub(self.limits.retention_secs)],
        )?;
        if let Some(status) = find_message(&tx, &message_id)? {
            tx.commit()?;
            return Ok(status);
        }
        if expires_at <= now {
            return Err(Error::Expired);
        }
        let bytes = message.encrypted_bytes() as u64;
        if bytes > self.limits.capacity_bytes {
            return Err(Error::PayloadTooLarge);
        }
        let count: i64 =
            tx.query_row("SELECT COUNT(*) FROM propagation_owned", [], |r| r.get(0))?;
        // Terminal records are acknowledged only after the application durably projects them.
        if count >= crate::MAX_STATUS as i64 {
            return Err(Error::StoreFull);
        }
        if let Some(key) = key {
            owned_changes::supersede(&tx, &message.destination, key, now)?;
        }
        owned_changes::make_room(&tx, bytes, self.limits.capacity_bytes, now)?;
        crate::storage::upsert_propagation_entry(
            &tx,
            &crate::storage::PropagationEntryRecord {
                transient_id: message.transient_id.clone(),
                destination: message.destination.clone(),
                payload_hex: hex::encode(&message.payload),
                received_at: now,
                size_bytes: message.payload.len() as u64,
                stamp_value: None,
            },
        )?;
        tx.execute("INSERT INTO propagation_owned(transient_id,message_id,destination,admitted_at,expires_at,
            recovery,encrypted_bytes,kind,replacement_key,state,changed_at,application_key) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,?4,?10)",
            params![message.transient_id,message_id,message.destination,now,expires_at,message.recovery,bytes as i64,kind,key,message.application_key])?;
        let status =
            find_message(&tx, &message_id)?.ok_or(Error::InvalidState("deposit missing"))?;
        tx.commit()?;
        Ok(status)
    }

    pub fn application_messages(&self, key: &str) -> Result<Vec<Status>, Error> {
        if key.len() > 128 {
            return Err(Error::InvalidState("invalid application key"));
        }
        let mut stmt=self.conn.prepare(&format!("SELECT {COLUMNS} FROM propagation_owned WHERE application_key=?1 ORDER BY admitted_at,transient_id LIMIT {}",crate::MAX_STATUS))?;
        let rows = stmt.query_map([key], from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn message(&self, message_id: &str) -> Result<Option<Status>, Error> {
        Ok(find_message(&self.conn, message_id)?)
    }

    pub fn status(&mut self, now: i64) -> Result<Vec<Status>, Error> {
        let tx = self.conn.unchecked_transaction()?;
        owned_changes::expire(&tx, now)?;
        let statuses = {
            let mut stmt=tx.prepare(&format!("SELECT {COLUMNS} FROM propagation_owned WHERE projected=0 ORDER BY admitted_at,transient_id LIMIT {}",crate::MAX_STATUS))?;
            let rows = stmt.query_map([], from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.commit()?;
        Ok(statuses)
    }

    /// The authorization callback is evaluated again immediately before exposing
    /// payloads to the transport. A single lease covers direct and relay recovery.
    pub fn recover(
        &mut self,
        id: &str,
        now: i64,
        retry_at: i64,
        permit: impl FnOnce(&Status) -> bool,
    ) -> Result<Option<Recovery>, Error> {
        self.ensure_running()?;
        if self.in_flight.is_some() {
            return Err(Error::InvalidState("recovery already active"));
        }
        // A former owner may have observed transport cleanup while SQLite was unavailable.
        self.conn.execute("UPDATE propagation_owned SET state=2, uploaded=1 WHERE state=1", [])?;
        let tx = self.conn.unchecked_transaction()?;
        owned_changes::expire(&tx, now)?;
        let status = find(&tx, id)?;
        let Some(mut status) = status.filter(|s| !s.state.terminal() && s.next_attempt_at <= now)
        else {
            tx.commit()?;
            return Ok(None);
        };
        if !permit(&status) {
            tx.commit()?;
            return Ok(None);
        }
        let recovery: Vec<u8> = tx.query_row(
            "SELECT recovery FROM propagation_owned WHERE transient_id=?1",
            [id],
            |r| r.get(0),
        )?;
        let entry = crate::storage::get_propagation_entry(&tx, id)?
            .ok_or(Error::InvalidState("custody payload missing"))?;
        let wire = prepared::recover_wire(
            &recovery,
            &self.identity,
            &status.message_id,
            &status.destination,
        )?;
        let payload = hex::decode(&entry.payload_hex)
            .map_err(|_| Error::InvalidState("invalid stored ciphertext"))?;
        status.attempts =
            status.attempts.checked_add(1).ok_or(Error::InvalidState("attempt bound exhausted"))?;
        status.next_attempt_at = retry_at.clamp(now.saturating_add(1), now.saturating_add(300));
        tx.execute(
            "UPDATE propagation_owned SET attempts=?2, next_attempt_at=?3 WHERE transient_id=?1",
            params![id, status.attempts, status.next_attempt_at],
        )?;
        tx.commit()?;
        let lease = RecoveryLease { transient_id: id.to_owned(), attempt: status.attempts };
        self.in_flight = Some(lease.clone());
        Ok(Some(Recovery { lease, status, direct_wire: wire, recipient_transient: payload }))
    }

    /// The host must authenticate this identity through its native Link, not request data.
    /// Permission is checked against the original signed wire before ciphertext is exposed.
    pub fn retrieve_for(
        &mut self,
        authenticated_identity: &rns_core::identity::Identity,
        now: i64,
        max_transfer_bytes: usize,
        mut permit: impl FnMut(&Status, &[u8]) -> bool,
    ) -> Result<Option<crate::RetrievedMessage>, Error> {
        self.ensure_running()?;
        if self.in_flight.is_some() {
            return Err(Error::InvalidState("recovery already active"));
        }
        let destination = hex::encode(prepared::delivery_address(authenticated_identity));
        let tx = self.conn.unchecked_transaction()?;
        owned_changes::expire(&tx, now)?;
        let statuses = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {COLUMNS} FROM propagation_owned WHERE destination=?1 AND state<4
                 AND transient_id IN (SELECT transient_id FROM propagation_entries WHERE destination=?1 AND size_bytes<=?2)
                 ORDER BY admitted_at,transient_id LIMIT {}", crate::MAX_PENDING,
            ))?;
            let rows = stmt.query_map(
                params![destination, i64::try_from(max_transfer_bytes).unwrap_or(i64::MAX)],
                from_row,
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for status in statuses {
            let entry = crate::storage::get_propagation_entry(&tx, &status.transient_id)?
                .ok_or(Error::InvalidState("custody payload missing"))?;
            if entry.payload_hex.len() / 2 > max_transfer_bytes {
                continue;
            }
            let recovery: Vec<u8> = tx.query_row(
                "SELECT recovery FROM propagation_owned WHERE transient_id=?1",
                [&status.transient_id],
                |row| row.get(0),
            )?;
            let wire = prepared::recover_wire(
                &recovery,
                &self.identity,
                &status.message_id,
                &status.destination,
            )?;
            if !permit(&status, &wire) {
                continue;
            }
            let recipient_transient = hex::decode(&entry.payload_hex)
                .map_err(|_| Error::InvalidState("invalid stored ciphertext"))?;
            tx.commit()?;
            return Ok(Some(crate::RetrievedMessage { status, recipient_transient }));
        }
        tx.commit()?;
        Ok(None)
    }

    pub fn finish_recovery(&mut self, lease: &RecoveryLease) -> Result<(), Error> {
        if self.in_flight.as_ref() == Some(lease) {
            let result = self.conn.execute("UPDATE propagation_owned SET state=2, uploaded=1 WHERE transient_id=?1 AND state=1",[lease.transient_id()]);
            self.in_flight = None;
            result?;
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.in_flight = None;
    }

    /// Acknowledge visibility while keeping the bounded original deduplication record.
    pub fn acknowledge_status(&mut self, id: &str) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE propagation_owned SET projected=1 WHERE transient_id=?1 AND state>=4",
            [id],
        )?;
        Ok(())
    }
    pub(crate) fn ensure_running(&self) -> Result<(), Error> {
        if self.stopped {
            Err(Error::Stopped)
        } else {
            Ok(())
        }
    }
}

pub(crate) const COLUMNS:&str="transient_id,message_id,destination,state,admitted_at,expires_at,encrypted_bytes,uploaded,relay,attempts,next_attempt_at,kind,replacement_key,application_key";
pub(crate) fn find(conn: &Connection, id: &str) -> rusqlite::Result<Option<Status>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM propagation_owned WHERE transient_id=?1"),
        [id],
        from_row,
    )
    .optional()
}
fn find_message(conn: &Connection, id: &str) -> rusqlite::Result<Option<Status>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM propagation_owned WHERE message_id=?1"),
        [id],
        from_row,
    )
    .optional()
}
fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Status> {
    Ok(Status {
        transient_id: row.get(0)?,
        message_id: row.get(1)?,
        application_key: row.get(13)?,
        destination: row.get(2)?,
        state: State::from_sql(row.get(3)?)?,
        kind: match row.get::<_, i64>(11)? {
            0 => MessageKind::Chat,
            1 => MessageKind::Application,
            2 => MessageKind::Telemetry { key: row.get(12)? },
            value => return Err(rusqlite::Error::IntegralValueOutOfRange(11, value)),
        },
        admitted_at: row.get(4)?,
        expires_at: row.get(5)?,
        encrypted_bytes: u64::from(row.get::<_, u32>(6)?),
        uploaded: row.get(7)?,
        relay: row.get(8)?,
        attempts: row.get(9)?,
        next_attempt_at: row.get(10)?,
    })
}
