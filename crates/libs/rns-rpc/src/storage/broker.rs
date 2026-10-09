//! Durable journal operations run on the existing messages writer, never a second writer.
use crate::broker::*;
use crate::storage::messages::MessageRecord;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct BrokerError {
    pub code: &'static str,
    pub message: String,
}
impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for BrokerError {}
pub fn error(code: &'static str, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(BrokerError { code, message: message.into() }))
}
fn encode<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs()
        .min(i64::MAX as u64) as i64
}
fn position(value: EventPosition) -> rusqlite::Result<i64> {
    i64::try_from(value.0).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct IssuedRange {
    start: EventPosition,
    end: EventPosition,
    receipt: StoredReceipt,
}

pub enum BrokerCommand {
    Resume { owner: String, destination: String, request: ResumeRequest },
    Fetch { owner: String, destination: String, request: FetchRequest },
    Ack { owner: String, destination: String, request: AckStoredRequest },
    Inbound { record: MessageRecord, raw_hex: Option<String> },
    Admit { owner: String, source: String, request: AdmitRequest },
    Reconcile { owner: String, source: String, request: ReconcileRequest },
    Prepared { message_id: String, payload: Value },
    DispatchState { message_id: String, state: String },
}

pub(crate) fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    let fields: Option<String> = row.get(7)?;
    Ok(MessageRecord {
        id: row.get(0)?,
        source: row.get(1)?,
        destination: row.get(2)?,
        title: row.get(3)?,
        content: row.get(4)?,
        timestamp: row.get(5)?,
        direction: row.get(6)?,
        fields: fields.as_deref().map(decode).transpose()?,
        receipt_status: row.get(8)?,
    })
}
fn local_destination(record: &MessageRecord) -> String {
    if record.direction == "in" {
        record.destination.to_ascii_lowercase()
    } else {
        record.source.to_ascii_lowercase()
    }
}

pub(crate) fn append(
    conn: &Connection,
    destination: &str,
    kind: &str,
    payload: &Value,
) -> rusqlite::Result<i64> {
    append_budgeted(conn, destination, kind, payload, false)
}
fn append_budgeted(
    conn: &Connection,
    destination: &str,
    kind: &str,
    payload: &Value,
    completion: bool,
) -> rusqlite::Result<i64> {
    let payload = encode(payload)?;
    // A single record must fit, with envelope, batch headers and receipt overhead.
    if payload.len() > MAX_BATCH_BYTES - 4096 {
        return Err(error(
            "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
            "event cannot fit in a legal broker response",
        ));
    }
    let (next, used, budget): (i64, i64, i64) =
        conn.query_row("SELECT next_position,used_bytes,budget FROM broker_meta", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let bytes = (payload.len() + destination.len() + kind.len() + 128) as i64;
    if !completion {
        physical_admission(conn, payload.len() as u64, budget as u64)?;
    }
    let limit = if completion { budget } else { budget.saturating_sub(512 * 1024) };
    if used.saturating_add(bytes) > limit {
        return Err(error(
            "SDK_STORAGE_BROKER_FULL",
            "unacknowledged journal reached its disk budget; ACK headroom reserved",
        ));
    }
    let successor = next
        .checked_add(1)
        .ok_or_else(|| error("SDK_STORAGE_BROKER_FULL", "event position exhausted"))?;
    conn.execute(
        "INSERT INTO broker_events VALUES(?1,?2,?3,?4,?5,?6)",
        params![next, destination, kind, now(), payload, bytes],
    )?;
    conn.execute(
        "UPDATE broker_meta SET next_position=?1,used_bytes=used_bytes+?2",
        params![successor, bytes],
    )?;
    Ok(next)
}

fn validate_key(value: &str) -> rusqlite::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(error("SDK_VALIDATION_CONSUMER_ID", "invalid consumer identifier"));
    }
    Ok(())
}
fn consumer(
    conn: &Connection,
    id: &str,
    owner: &str,
    destination: &str,
) -> rusqlite::Result<Option<(i64, Option<String>)>> {
    let record: Option<(String, String, i64, Option<String>)> = conn
        .query_row(
            "SELECT owner,destination,stored,issued FROM broker_consumers WHERE consumer_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    match record {
        Some((o, d, b, issued)) if o == owner && d == destination => Ok(Some((b, issued))),
        Some(_) => Err(error(
            "SDK_SECURITY_CONSUMER_FORBIDDEN",
            "consumer belongs to a different principal or service identity",
        )),
        None => Ok(None),
    }
}
fn require_consumer(
    conn: &Connection,
    id: &str,
    owner: &str,
    destination: &str,
) -> rusqlite::Result<(i64, Option<String>)> {
    validate_key(id)?;
    consumer(conn, id, owner, destination)?.ok_or_else(|| {
        error("SDK_BROKER_SESSION_REQUIRED", "resume the consumer before fetching or acknowledging")
    })
}
fn mac(
    conn: &Connection,
    journal: &str,
    id: &str,
    owner: &str,
    destination: &str,
    start: i64,
    end: i64,
) -> rusqlite::Result<Hmac<Sha256>> {
    let secret: Vec<u8> = conn.query_row("SELECT secret FROM broker_meta", [], |r| r.get(0))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    mac.update(encode(&(journal, id, owner, destination, start, end))?.as_bytes());
    Ok(mac)
}
fn receipt(
    conn: &Connection,
    journal: &str,
    id: &str,
    owner: &str,
    destination: &str,
    start: i64,
    end: i64,
) -> rusqlite::Result<String> {
    Ok(format!(
        "{start}:{end}:{}",
        hex::encode(
            mac(conn, journal, id, owner, destination, start, end)?.finalize().into_bytes()
        )
    ))
}
fn verify_receipt(
    conn: &Connection,
    journal: &str,
    id: &str,
    owner: &str,
    destination: &str,
    range: (i64, i64),
    tag: &str,
) -> rusqlite::Result<()> {
    let tag = hex::decode(tag).map_err(|_| error("SDK_BROKER_INVALID_RECEIPT", "invalid MAC"))?;
    mac(conn, journal, id, owner, destination, range.0, range.1)?
        .verify_slice(&tag)
        .map_err(|_| error("SDK_BROKER_INVALID_RECEIPT", "receipt authentication failed"))
}
fn prune(conn: &Connection) -> rusqlite::Result<()> {
    let floor: Option<i64> =
        conn.query_row("SELECT MIN(stored) FROM broker_consumers", [], |r| r.get(0))?;
    if let Some(floor) = floor {
        let bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(bytes),0) FROM broker_events WHERE position<=?1",
            [floor],
            |r| r.get(0),
        )?;
        conn.execute("DELETE FROM broker_events WHERE position<=?1", [floor])?;
        conn.execute(
            "UPDATE broker_meta SET floor=MAX(floor,?1),used_bytes=used_bytes-?2",
            params![floor, bytes],
        )?;
    }
    Ok(())
}
pub(crate) fn insert_atomic(
    conn: &Connection,
    record: &MessageRecord,
    raw_hex: Option<&str>,
) -> rusqlite::Result<bool> {
    insert_with_operation(conn, record, raw_hex, None)
}
fn insert_with_operation(
    conn: &Connection,
    record: &MessageRecord,
    raw_hex: Option<&str>,
    operation: Option<&str>,
) -> rusqlite::Result<bool> {
    let fingerprint = message_fingerprint(record)?;
    let prior: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM broker_message_keys WHERE message_id=?1",
            [&record.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        if prior != fingerprint {
            return Err(error(
                "SDK_STORAGE_MESSAGE_CONFLICT",
                "duplicate identifier has different immutable content",
            ));
        }
        return Ok(false);
    }
    let existing = conn.query_row("SELECT id,source,destination,title,content,timestamp,direction,fields,receipt_status FROM messages WHERE id=?1",[&record.id],record_from_row).optional()?;
    if let Some(existing) = existing {
        // Receipt/status and private bridge metadata may advance independently.
        if existing.source != record.source
            || existing.destination != record.destination
            || existing.title != record.title
            || existing.content != record.content
            || existing.timestamp != record.timestamp
            || existing.direction != record.direction
        {
            return Err(error(
                "SDK_STORAGE_MESSAGE_CONFLICT",
                "LXMF message identifier has conflicting immutable content",
            ));
        }
        return Ok(false);
    }
    mutation_admission(conn, true)?;
    charge(conn, (record.id.len() + fingerprint.len() + 128) as i64)?;
    conn.execute("INSERT INTO broker_message_keys VALUES(?1,?2)", params![record.id, fingerprint])?;
    reserve_completion(conn, record)?;
    let fields = record.fields.as_ref().map(encode).transpose()?;
    conn.execute("INSERT INTO messages (id,source,destination,title,content,timestamp,direction,fields,receipt_status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![record.id,record.source,record.destination,record.title,record.content,record.timestamp,record.direction,fields,record.receipt_status])?;
    let mut payload = json!({"message": record});
    if let Some(raw) = raw_hex {
        payload["lxmf_bytes_hex"] = json!(raw);
    }
    if let Some(operation) = operation {
        payload["operation_id"] = json!(operation);
    }
    append(
        conn,
        &local_destination(record),
        if record.direction == "in" { "inbound" } else { "outbound" },
        &payload,
    )?;
    Ok(true)
}

pub(crate) fn status_atomic(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<()> {
    let record = conn.query_row("SELECT id,source,destination,title,content,timestamp,direction,fields,receipt_status FROM messages WHERE id=?1",[id],record_from_row).optional()?;
    if let Some(mut record) = record {
        if record.receipt_status.as_deref().is_some_and(|existing| {
            existing == status
                || crate::storage::messages::MessagesStore::should_preserve_receipt_status(
                    existing, status,
                )
        }) {
            return Ok(());
        }
        record.receipt_status = Some(status.into());
        let terminal = crate::storage::messages::MessagesStore::is_terminal_receipt_status(status);
        let reservation: Option<i64> = conn
            .query_row("SELECT bytes FROM broker_completions WHERE message_id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        mutation_admission(conn, !(terminal && reservation.is_some()))?;
        conn.execute("UPDATE messages SET receipt_status=?1 WHERE id=?2", params![status, id])?;
        if terminal {
            let unused: Option<i64> = conn
                .query_row(
                    "SELECT prepared_reserve FROM broker_operations WHERE message_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(unused) = unused {
                conn.execute("UPDATE broker_meta SET used_bytes=used_bytes-?1", [unused])?;
            }
            conn.execute("UPDATE broker_operations SET state='terminal',prepared_reserve=0 WHERE message_id=?1",[id])?;
            if let Some(bytes) = reservation {
                conn.execute("UPDATE broker_meta SET used_bytes=used_bytes-?1", [bytes])?;
                conn.execute("DELETE FROM broker_completions WHERE message_id=?1", [id])?;
            }
        }
        let reason: String = status.chars().take(512).collect();
        let payload = json!({"message_id":id,"status":reason,"reason_truncated":reason.len()!=status.len(),"message":{"id":record.id,"source":record.source,"destination":record.destination,"direction":record.direction}});
        append_budgeted(
            conn,
            &local_destination(&record),
            "receipt",
            &payload,
            terminal && reservation.is_some(),
        )?;
    }
    Ok(())
}

fn charge(conn: &Connection, bytes: i64) -> rusqlite::Result<()> {
    mutation_admission(conn, true)?;
    let (used, budget): (i64, i64) =
        conn.query_row("SELECT used_bytes,budget FROM broker_meta", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    if used.saturating_add(bytes) > budget {
        return Err(error("SDK_STORAGE_BROKER_FULL", "durable storage budget exhausted"));
    }
    conn.execute("UPDATE broker_meta SET used_bytes=used_bytes+?1", [bytes])?;
    Ok(())
}

fn replay_batch(
    conn: &Connection,
    consumer: &ConsumerId,
    destination: &str,
    range: IssuedRange,
) -> rusqlite::Result<BrokerBatch> {
    let journal: String = conn.query_row("SELECT journal_id FROM broker_meta", [], |r| r.get(0))?;
    let mut statement=conn.prepare("SELECT position,event_type,created_at,payload FROM broker_events WHERE position>?1 AND position<=?2 AND local_destination=?3 ORDER BY position LIMIT ?4")?;
    let rows = statement.query_map(
        params![position(range.start)?, position(range.end)?, destination, MAX_BATCH_EVENTS as i64],
        |row| {
            let payload: String = row.get(3)?;
            Ok(BrokerEvent {
                version: 1,
                position: EventPosition(row.get::<_, i64>(0)? as u64),
                event_type: row.get(1)?,
                created_at: row.get(2)?,
                payload: decode(&payload)?,
            })
        },
    )?;
    Ok(BrokerBatch {
        journal_id: JournalId(journal),
        consumer_id: consumer.clone(),
        start: range.start,
        end: range.end,
        receipt: range.receipt,
        events: rows.collect::<rusqlite::Result<_>>()?,
    })
}

fn message_fingerprint(record: &MessageRecord) -> rusqlite::Result<String> {
    let mut fields = record.fields.clone();
    if let Some(object) = fields.as_mut().and_then(Value::as_object_mut) {
        object.remove("_lxmf");
    }
    Ok(hex::encode(Sha256::digest(
        encode(&(
            record.source.as_str(),
            record.destination.as_str(),
            record.title.as_str(),
            record.content.as_str(),
            record.timestamp,
            record.direction.as_str(),
            fields,
        ))?
        .as_bytes(),
    )))
}

fn reserve_completion(conn: &Connection, record: &MessageRecord) -> rusqlite::Result<()> {
    if record.direction != "out"
        || record
            .receipt_status
            .as_deref()
            .is_some_and(crate::storage::messages::MessagesStore::is_terminal_receipt_status)
    {
        return Ok(());
    }
    let bytes = (record.id.len() + record.source.len() + record.destination.len())
        .saturating_mul(6)
        .saturating_add(8192) as i64;
    charge(conn, bytes)?;
    conn.execute("INSERT INTO broker_completions VALUES(?1,?2)", params![record.id, bytes])?;
    Ok(())
}

mod execution;
mod physical_guard;
pub(crate) use physical_guard::{
    map_commit_error, mutation as mutation_admission, single_transaction,
};
mod profile;
pub use execution::execute;
use profile::physical_admission;
pub use profile::{enable, enabled, restore_profile};

pub(crate) use profile::reclaim_wal;
