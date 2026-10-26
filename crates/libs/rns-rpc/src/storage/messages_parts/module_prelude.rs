use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use serde_json::Value as JsonValue;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
    durable_broker_enabled: AtomicBool,
    write_state: Arc<WriteState>,
    outbound_write_tx: WriteSender,
    writer_thread: Option<JoinHandle<()>>,
    read_conn: Option<Mutex<Connection>>,
    maintenance_conn: Option<Mutex<Connection>>,
    read_lock_wait_ns_total: AtomicU64,
    read_ops_total: AtomicU64,
}

struct WriteState {
    conn: Mutex<Connection>,
    message_count_cache: AtomicU64,
    write_lock_wait_ns_total: AtomicU64,
    write_ops_total: AtomicU64,
    refill_metrics: super::operation_metrics::RefillMetrics,
    wal_metrics: super::operation_metrics::WalMetrics,
    /// Broker/control writers currently waiting for `conn`; refill pages yield to them.
    priority_waiters: AtomicUsize,
    /// Signalled once a priority writer holds `conn`, so a yielding refill wakes
    /// without polling.
    priority_gate: (Mutex<()>, Condvar),
    /// Cached `broker_meta.budget` (0 = broker disabled) so the per-write WAL
    /// emergency check needs no SQL.
    broker_budget_bytes: AtomicU64,
    last_wal_reclaim: Mutex<Option<std::time::Instant>>,
    wal_reclaim_interval: std::time::Duration,
}

enum OutboundWriteCommand {
    Broker {
        command: Box<super::broker::BrokerCommand>,
        reply: mpsc::Sender<rusqlite::Result<JsonValue>>,
    },
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

pub use lxmf_propagation::storage::{PropagationEntryRecord, PropagationEntryStats};
use lxmf_propagation::storage::propagation_entry_from_row;

pub const LXMF_LOCAL_TRANSIENT_CACHE_EXPIRY_SECS: i64 = 30 * 24 * 60 * 60 * 6;

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

fn sql_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}
