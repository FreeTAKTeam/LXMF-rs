//! Durable journal operations run on the existing messages writer, never a second writer.
use crate::broker::*;
use crate::storage::messages::MessageRecord;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use r3akt_event_broker::storage::{append, append_budgeted, charge, event_bytes, validate_key};
pub use r3akt_event_broker::storage::{error, BrokerError};
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

pub enum BrokerCommand {
    Resume { owner: String, destination: String, request: ResumeRequest },
    Fetch { owner: String, destination: String, request: FetchRequest },
    Ack { owner: String, destination: String, request: AckStoredRequest },
    Inbound { record: MessageRecord, raw_hex: Option<String> },
    Admit { owner: String, source: String, request: AdmitRequest },
    Reconcile { owner: String, source: String, request: ReconcileRequest },
    Prepared { message_id: String, payload: Value },
    ReservePropagationCustody { message_id: String },
    DispatchState { message_id: String, state: String },
    PropagationCustody { message_id: String, representation: PropagationCustody },
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

mod receipt;
pub(crate) use receipt::status_atomic;

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
pub(crate) use r3akt_event_broker::storage::physical_guard::{
    map_commit_error, mutation as mutation_admission, single_transaction,
};
mod profile;
mod wal_maintenance;
pub use execution::execute;
use profile::physical_admission;
pub use profile::{enable, enabled, restore_profile};
pub(crate) use wal_maintenance::{
    checkpoint_truncate_nonblocking, maintain as wal_maintenance_run, wal_file_size,
};
pub use wal_maintenance::{truncate_limit_bytes, wal_maintenance_interval, WalMaintenanceOutcome};
