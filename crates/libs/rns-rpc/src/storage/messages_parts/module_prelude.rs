use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use serde_json::Value as JsonValue;

use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::{mpsc, Arc, Mutex};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MessageRecord {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub title: String,
    pub content: String,
    pub timestamp: i64,
    pub direction: String,
    pub fields: Option<JsonValue>,
    pub receipt_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnnounceRecord {
    pub id: String,
    pub peer: String,
    pub timestamp: i64,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub first_seen: i64,
    pub seen_count: u64,
    pub app_data_hex: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub rssi: Option<f64>,
    pub snr: Option<f64>,
    pub q: Option<f64>,
    pub stamp_cost: Option<u32>,
    pub stamp_cost_flexibility: Option<u32>,
    pub peering_cost: Option<u32>,
}

pub struct MessagesStore {
    write_state: Arc<WriteState>,
    outbound_write_tx: mpsc::Sender<OutboundWriteCommand>,
    read_conn: Option<Mutex<Connection>>,
    read_lock_wait_ns_total: AtomicU64,
    read_ops_total: AtomicU64,
}

struct WriteState {
    conn: Mutex<Connection>,
    message_count_cache: AtomicU64,
    write_lock_wait_ns_total: AtomicU64,
    write_ops_total: AtomicU64,
}

enum OutboundWriteCommand {
    InsertMessage {
        record: MessageRecord,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    ResolveReceiptStatus {
        message_id: String,
        candidate_status: String,
        reply: mpsc::Sender<rusqlite::Result<Option<String>>>,
    },
    UpdateReceiptStatus {
        message_id: String,
        status: String,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    UpdateMessageFields {
        message_id: String,
        fields_json: Option<String>,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    UpsertAnnounceIdentity {
        peer: String,
        public_key_hex: String,
        verifying_key_hex: String,
        updated_at: i64,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    InsertAnnounce {
        record: AnnounceRecord,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    UpsertTicket {
        destination: String,
        ticket: String,
        expires_at: i64,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    PruneExpiredTickets {
        now: i64,
        inbound_grace_secs: i64,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    UpsertOutboundTicket {
        destination: String,
        ticket: String,
        expires_at: i64,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    UpsertTicketLastDelivery {
        destination: String,
        delivered_at: i64,
        reply: mpsc::Sender<rusqlite::Result<()>>,
    },
    PruneMessagesToLimitBytes {
        limit_bytes: u64,
        reply: Option<mpsc::Sender<rusqlite::Result<Vec<String>>>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagesStoreContentionSnapshot {
    pub read_lock_wait_ns_total: u64,
    pub read_ops_total: u64,
    pub write_lock_wait_ns_total: u64,
    pub write_ops_total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageStorageStats {
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerMessageStats {
    pub outgoing: u64,
    pub incoming: u64,
    pub offered: u64,
    pub unhandled: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerPropagationMessageStats {
    pub outgoing: u64,
    pub incoming: u64,
    pub offered: u64,
    pub unhandled: u64,
    pub offered_bytes: u64,
    pub unhandled_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropagationEntryRecord {
    pub transient_id: String,
    pub destination: String,
    pub payload_hex: String,
    pub received_at: i64,
    pub size_bytes: u64,
    pub stamp_value: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropagationEntryStats {
    pub entries: u64,
    pub bytes: u64,
}

fn propagation_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PropagationEntryRecord> {
    let size_bytes: i64 = row.get(4)?;
    let stamp_value: Option<u32> = row.get(5)?;
    Ok(PropagationEntryRecord {
        transient_id: row.get(0)?,
        destination: row.get(1)?,
        payload_hex: row.get(2)?,
        received_at: row.get(3)?,
        size_bytes: size_bytes.max(0) as u64,
        stamp_value,
    })
}

fn propagation_prune_weight(
    destination: &str,
    size_bytes: u64,
    received_at: i64,
    newest_received_at: Option<i64>,
    prioritised_destinations: &[String],
) -> f64 {
    const FOUR_DAYS_SECS: f64 = 4.0 * 24.0 * 60.0 * 60.0;

    let age_secs = newest_received_at.unwrap_or(received_at).saturating_sub(received_at) as f64;
    let age_weight = (age_secs / FOUR_DAYS_SECS).max(1.0);
    let priority_weight = if prioritised_destinations
        .iter()
        .any(|candidate| destination.eq_ignore_ascii_case(candidate.trim()))
    {
        0.1
    } else {
        1.0
    };
    priority_weight * age_weight * size_bytes as f64
}

fn normalize_hex_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_peer_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn remove_case_variant_unhandled_peer_mark(
    conn: &Connection,
    peer: &str,
    transient_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM propagation_peer_entries
         WHERE LOWER(peer) = LOWER(?1)
           AND transient_id = ?2
           AND state = 'unhandled'",
        params![peer, transient_id],
    )?;
    Ok(())
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}
