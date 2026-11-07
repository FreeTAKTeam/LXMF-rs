//! Durable primitives borrowing an application-owned SQLite transaction.
//! These functions do not commit: related application mutations must share the transaction.
use crate::*;
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::Sha256;

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

pub fn append(
    conn: &Connection,
    destination: &str,
    kind: &str,
    payload: &Value,
) -> rusqlite::Result<i64> {
    append_budgeted(conn, destination, kind, payload, false)
}
pub fn append_budgeted(
    conn: &Connection,
    destination: &str,
    kind: &str,
    payload: &Value,
    completion: bool,
) -> rusqlite::Result<i64> {
    append_with_limits(conn, destination, kind, payload, completion, Limits::default())
}

/// Append under explicit bounds; `completion` spends previously admitted control capacity.
pub fn append_with_limits(
    conn: &Connection,
    destination: &str,
    kind: &str,
    payload: &Value,
    completion: bool,
    limits: Limits,
) -> rusqlite::Result<i64> {
    limits.validate()?;
    let payload = encode(payload)?;
    // A single record must fit, with envelope, batch headers and receipt overhead.
    if payload.len() > limits.max_batch_bytes - 4096 {
        return Err(error(
            "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
            "event cannot fit in a legal broker response",
        ));
    }
    let (next, used, budget): (i64, i64, i64) =
        conn.query_row("SELECT next_position,used_bytes,budget FROM broker_meta", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let bytes = event_bytes(destination, kind, &payload);
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

/// Logical accounting for an encoded event, also used to size completion reservations.
pub fn event_bytes(destination: &str, kind: &str, encoded_payload: &str) -> i64 {
    (encoded_payload.len() + destination.len() + kind.len() + 128) as i64
}

pub fn validate_key(value: &str) -> rusqlite::Result<()> {
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
/// Retain metadata/reservations in the existing logical budget.
pub fn charge(conn: &Connection, bytes: i64) -> rusqlite::Result<()> {
    if bytes < 0 {
        return Err(error("SDK_VALIDATION_BROKER_BUDGET", "negative retained charge"));
    }
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
    let mut statement=conn.prepare("SELECT position,event_type,created_at,payload FROM broker_events WHERE position>?1 AND position<=?2 AND local_destination=?3 ORDER BY position")?;
    let rows = statement.query_map(
        params![position(range.start)?, position(range.end)?, destination],
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

mod consumer;
pub mod physical_guard;
mod profile;
pub mod wal_maintenance;
pub use consumer::{ack, fetch, fetch_with_limits, resume, subscribe};
use physical_guard::mutation as mutation_admission;
pub use profile::{
    configure_profile, configure_profile_with_limits, enabled, initialize_schema,
    physical_admission,
};

/// Release an existing reservation using control headroom, inside the caller's transaction.
pub fn release_charge(conn: &Connection, bytes: i64) -> rusqlite::Result<()> {
    if bytes < 0 {
        return Err(error("SDK_VALIDATION_BROKER_BUDGET", "negative released charge"));
    }
    mutation_admission(conn, false)?;
    let changed = conn
        .execute("UPDATE broker_meta SET used_bytes=used_bytes-?1 WHERE used_bytes>=?1", [bytes])?;
    if changed != 1 {
        return Err(error(
            "SDK_STORAGE_BROKER_ACCOUNTING",
            "reservation exceeds retained accounting",
        ));
    }
    Ok(())
}
