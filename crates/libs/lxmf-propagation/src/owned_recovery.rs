use crate::{owned, owned_changes, Error, OwnedStore, RecoveryLease, State};
use rusqlite::params;

impl OwnedStore {
    pub fn begin_upload(
        &mut self,
        lease: &RecoveryLease,
        relay: &str,
        now: i64,
    ) -> Result<(), Error> {
        self.ensure_running()?;
        let id = lease.transient_id();
        if self.in_flight.as_ref() != Some(lease) {
            return Err(Error::InvalidState("recovery lease required"));
        }
        if relay.len() != 32 || hex::decode(relay).is_err() {
            return Err(Error::InvalidState("invalid relay"));
        }
        let tx = self.conn.unchecked_transaction()?;
        let status = owned::find(&tx, id)?.ok_or(Error::InvalidState("unknown custody"))?;
        if status.expires_at <= now {
            return Err(Error::Expired);
        }
        if status.state.terminal() {
            return Err(Error::InvalidState("custody already retired"));
        }
        if status.uploaded {
            return Err(Error::InvalidState("relay handoff already attempted"));
        }
        tx.execute(
            "UPDATE propagation_owned SET state=1, relay=?2, changed_at=?3 WHERE transient_id=?1",
            params![id, relay, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Link/resource success cannot prove a stock relay's durable application receipt.
    /// Keep both encrypted payloads and suppress repeat upload to the same relay.
    pub fn uploaded_unverified(&mut self, lease: &RecoveryLease, now: i64) -> Result<(), Error> {
        let id = lease.transient_id();
        if self.in_flight.as_ref() != Some(lease) {
            return Err(Error::InvalidState("stale recovery lease"));
        }
        let tx = self.conn.unchecked_transaction()?;
        let status = owned::find(&tx, id)?.ok_or(Error::InvalidState("unknown custody"))?;
        if status.state != State::Uploading {
            return Err(Error::InvalidState("upload not active"));
        }
        if status.expires_at <= now {
            owned_changes::retire(&tx, id, State::Expired, now)?;
        } else {
            tx.execute("UPDATE propagation_owned SET state=2, uploaded=1, changed_at=?2 WHERE transient_id=?1",params![id,now])?;
        }
        tx.commit()?;
        self.in_flight = None;
        Ok(())
    }

    /// The caller supplies the original message ID from a validated recipient proof.
    /// A stale acknowledgement cannot retire a newer telemetry replacement.
    pub fn delivered(
        &mut self,
        id: &str,
        message_id: &str,
        acknowledged: bool,
        now: i64,
    ) -> Result<bool, Error> {
        let tx = self.conn.unchecked_transaction()?;
        owned_changes::expire(&tx, now)?;
        let Some(status) = owned::find(&tx, id)? else {
            tx.commit()?;
            return Ok(false);
        };
        if status.message_id != message_id
            || matches!(status.state, State::Expired | State::Evicted)
        {
            tx.commit()?;
            return Ok(false);
        }
        let state = if acknowledged { State::Acknowledged } else { State::Delivered };
        if status.state == State::Acknowledged || status.state == state {
            tx.commit()?;
            return Ok(true);
        }
        owned_changes::retire(&tx, id, state, now)?;
        tx.commit()?;
        if self.in_flight.as_ref().is_some_and(|lease| lease.transient_id() == id) {
            self.in_flight = None;
        }
        Ok(true)
    }
}
