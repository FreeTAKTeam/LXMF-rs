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

impl MessagesStore {
    const SDK_DOMAIN_SNAPSHOT_KEY: &'static str = "sdk_domains.v1";

    fn is_terminal_receipt_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("failed")
            || matches!(normalized.as_str(), "cancelled" | "delivered" | "expired" | "rejected")
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let write_state = Arc::new(WriteState {
            conn: Mutex::new(conn),
            message_count_cache: AtomicU64::new(0),
            write_lock_wait_ns_total: AtomicU64::new(0),
            write_ops_total: AtomicU64::new(0),
        });
        let (outbound_write_tx, outbound_write_rx) = mpsc::channel();
        let store = Self {
            write_state: write_state.clone(),
            outbound_write_tx,
            read_conn: None,
            read_lock_wait_ns_total: AtomicU64::new(0),
            read_ops_total: AtomicU64::new(0),
        };
        store.configure_connection()?;
        store.init_schema()?;
        store.refresh_message_count_cache()?;
        Self::spawn_outbound_write_worker(write_state, outbound_write_rx);
        Ok(store)
    }

    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let write_conn = Connection::open(path)?;
        let read_conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        let write_state = Arc::new(WriteState {
            conn: Mutex::new(write_conn),
            message_count_cache: AtomicU64::new(0),
            write_lock_wait_ns_total: AtomicU64::new(0),
            write_ops_total: AtomicU64::new(0),
        });
        let (outbound_write_tx, outbound_write_rx) = mpsc::channel();
        let store = Self {
            write_state: write_state.clone(),
            outbound_write_tx,
            read_conn: Some(Mutex::new(read_conn)),
            read_lock_wait_ns_total: AtomicU64::new(0),
            read_ops_total: AtomicU64::new(0),
        };
        store.configure_connection()?;
        store.init_schema()?;
        store.refresh_message_count_cache()?;
        Self::spawn_outbound_write_worker(write_state, outbound_write_rx);
        Ok(store)
    }

    fn refresh_message_count_cache(&self) -> rusqlite::Result<()> {
        let count: i64 = self.with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        })?;
        self.write_state
            .message_count_cache
            .store(count.max(0) as u64, Ordering::Relaxed);
        Ok(())
    }

    fn spawn_outbound_write_worker(
        write_state: Arc<WriteState>,
        rx: mpsc::Receiver<OutboundWriteCommand>,
    ) {
        std::thread::Builder::new()
            .name("messages-outbound-writer".to_string())
            .spawn(move || {
                while let Ok(command) = rx.recv() {
                    match command {
                        OutboundWriteCommand::InsertMessage { record, reply } => {
                            let _ = reply.send(Self::insert_message_direct(
                                write_state.as_ref(),
                                &record,
                            ));
                        }
                        OutboundWriteCommand::ResolveReceiptStatus {
                            message_id,
                            candidate_status,
                            reply,
                        } => {
                            let _ = reply.send(Self::resolve_receipt_status_direct(
                                write_state.as_ref(),
                                message_id.as_str(),
                                candidate_status.as_str(),
                            ));
                        }
                    }
                }
            })
            .expect("spawn messages outbound writer");
    }

    fn with_write_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let started = std::time::Instant::now();
        let conn = self.write_state.conn.lock().expect("messages sqlite write mutex poisoned");
        let waited_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.write_state
            .write_lock_wait_ns_total
            .fetch_add(waited_ns, Ordering::Relaxed);
        self.write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
        f(&conn)
    }

    fn with_read_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        if let Some(conn) = &self.read_conn {
            let started = std::time::Instant::now();
            let conn = conn.lock().expect("messages sqlite read mutex poisoned");
            let waited_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            self.read_lock_wait_ns_total.fetch_add(waited_ns, Ordering::Relaxed);
            self.read_ops_total.fetch_add(1, Ordering::Relaxed);
            f(&conn)
        } else {
            self.with_write_conn(f)
        }
    }

    pub fn contention_snapshot(&self) -> MessagesStoreContentionSnapshot {
        MessagesStoreContentionSnapshot {
            read_lock_wait_ns_total: self.read_lock_wait_ns_total.load(Ordering::Relaxed),
            read_ops_total: self.read_ops_total.load(Ordering::Relaxed),
            write_lock_wait_ns_total: self
                .write_state
                .write_lock_wait_ns_total
                .load(Ordering::Relaxed),
            write_ops_total: self.write_state.write_ops_total.load(Ordering::Relaxed),
        }
    }

    fn write_lock_and_run<T>(
        write_state: &WriteState,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let started = std::time::Instant::now();
        let conn = write_state.conn.lock().expect("messages sqlite write mutex poisoned");
        let waited_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        write_state
            .write_lock_wait_ns_total
            .fetch_add(waited_ns, Ordering::Relaxed);
        write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
        f(&conn)
    }

    fn insert_message_direct(write_state: &WriteState, record: &MessageRecord) -> rusqlite::Result<()> {
        let fields_json =
            record.fields.as_ref().map(|value| serde_json::to_string(value).unwrap_or_default());
        Self::write_lock_and_run(write_state, |conn| {
            let inserted = conn.execute(
                "INSERT INTO messages (id, source, destination, title, content, timestamp, direction, fields, receipt_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    &record.id,
                    &record.source,
                    &record.destination,
                    &record.title,
                    &record.content,
                    record.timestamp,
                    &record.direction,
                    fields_json,
                    &record.receipt_status,
                ],
            )?;
            if inserted == 0 {
                conn.execute(
                    "UPDATE messages
                     SET source = ?2,
                         destination = ?3,
                         title = ?4,
                         content = ?5,
                         timestamp = ?6,
                         direction = ?7,
                         fields = ?8,
                         receipt_status = ?9
                     WHERE id = ?1",
                    params![
                        &record.id,
                        &record.source,
                        &record.destination,
                        &record.title,
                        &record.content,
                        record.timestamp,
                        &record.direction,
                        fields_json,
                        &record.receipt_status,
                    ],
                )?;
            } else {
                write_state.message_count_cache.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        })
    }

    fn resolve_receipt_status_direct(
        write_state: &WriteState,
        message_id: &str,
        candidate_status: &str,
    ) -> rusqlite::Result<Option<String>> {
        Self::write_lock_and_run(write_state, |conn| {
            let existing_status = conn
                .query_row(
                    "SELECT receipt_status FROM messages WHERE id = ?1 LIMIT 1",
                    params![message_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten();
            if let Some(existing_status) = existing_status {
                if Self::is_terminal_receipt_status(existing_status.as_str()) {
                    return Ok(Some(existing_status));
                }
            }
            conn.execute(
                "UPDATE messages SET receipt_status = ?1 WHERE id = ?2",
                params![candidate_status, message_id],
            )?;
            Ok(Some(candidate_status.to_string()))
        })
    }

    pub fn insert_message(&self, record: &MessageRecord) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::InsertMessage {
                record: record.clone(),
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn list_messages(
        &self,
        limit: usize,
        before_ts: Option<i64>,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        self.with_read_conn(|conn| {
            let mut records = Vec::new();
            if let Some(ts) = before_ts {
                let mut stmt = conn.prepare(
                    "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE timestamp < ?1 ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![ts, limit as i64])?;
                while let Some(row) = rows.next()? {
                    let fields_json: Option<String> = row.get(7)?;
                    let fields =
                        fields_json.as_ref().and_then(|value| serde_json::from_str(value).ok());
                    let receipt_status: Option<String> = row.get(8)?;
                    records.push(MessageRecord {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        destination: row.get(2)?,
                        title: row.get(3)?,
                        content: row.get(4)?,
                        timestamp: row.get(5)?,
                        direction: row.get(6)?,
                        fields,
                        receipt_status,
                    });
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages ORDER BY timestamp DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit as i64])?;
                while let Some(row) = rows.next()? {
                    let fields_json: Option<String> = row.get(7)?;
                    let fields =
                        fields_json.as_ref().and_then(|value| serde_json::from_str(value).ok());
                    let receipt_status: Option<String> = row.get(8)?;
                    records.push(MessageRecord {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        destination: row.get(2)?,
                        title: row.get(3)?,
                        content: row.get(4)?,
                        timestamp: row.get(5)?,
                        direction: row.get(6)?,
                        fields,
                        receipt_status,
                    });
                }
            }
            Ok(records)
        })
    }

    pub fn get_message(&self, message_id: &str) -> rusqlite::Result<Option<MessageRecord>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE id = ?1 LIMIT 1",
            )?;
            stmt.query_row(params![message_id], |row| {
                let fields_json: Option<String> = row.get(7)?;
                let fields =
                    fields_json.as_ref().and_then(|value| serde_json::from_str(value).ok());
                let receipt_status: Option<String> = row.get(8)?;
                Ok(MessageRecord {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    destination: row.get(2)?,
                    title: row.get(3)?,
                    content: row.get(4)?,
                    timestamp: row.get(5)?,
                    direction: row.get(6)?,
                    fields,
                    receipt_status,
                })
            })
            .optional()
        })
    }

    pub fn message_count(&self) -> rusqlite::Result<u64> {
        Ok(self.write_state.message_count_cache.load(Ordering::Relaxed))
    }

    pub fn message_storage_stats(&self) -> rusqlite::Result<MessageStorageStats> {
        self.with_read_conn(|conn| {
            let count = self.write_state.message_count_cache.load(Ordering::Relaxed);
            let bytes: Option<i64> = conn.query_row(
                "SELECT COALESCE(SUM(
                    LENGTH(id) +
                    LENGTH(source) +
                    LENGTH(destination) +
                    LENGTH(title) +
                    LENGTH(content) +
                    LENGTH(direction) +
                    COALESCE(LENGTH(fields), 0) +
                    COALESCE(LENGTH(receipt_status), 0)
                ), 0) FROM messages",
                [],
                |row| row.get(0),
            )?;
            Ok(MessageStorageStats {
                count,
                bytes: bytes.unwrap_or(0).max(0) as u64,
            })
        })
    }

    pub fn peer_message_stats(&self, peer: &str) -> rusqlite::Result<PeerMessageStats> {
        self.with_read_conn(|conn| {
            let (outgoing, incoming, offered, unhandled): (i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN destination = ?1 AND direction = 'out' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN source = ?1 AND direction = 'in' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN destination = ?1
                         AND direction = 'out'
                         AND (
                            receipt_status IS NULL
                            OR TRIM(receipt_status) = ''
                            OR (
                                LOWER(receipt_status) NOT LIKE 'sent%'
                                AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                            )
                         )
                        THEN 1
                        ELSE 0
                    END), 0),
                    COALESCE(SUM(CASE WHEN source = ?1 AND direction = 'in' AND receipt_status IS NULL THEN 1 ELSE 0 END), 0)
                 FROM messages",
                params![peer],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            Ok(PeerMessageStats {
                outgoing: outgoing.max(0) as u64,
                incoming: incoming.max(0) as u64,
                offered: offered.max(0) as u64,
                unhandled: unhandled.max(0) as u64,
            })
        })
    }

    pub fn count_message_buckets(&self) -> rusqlite::Result<(u64, u64)> {
        let (queued, in_flight): (i64, i64) = self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT
                    COALESCE(SUM(CASE
                        WHEN receipt_status IS NULL OR TRIM(receipt_status) = '' THEN 1
                        ELSE 0
                    END), 0) AS queued_count,
                    COALESCE(SUM(CASE
                        WHEN receipt_status IS NOT NULL
                            AND TRIM(receipt_status) <> ''
                            AND LOWER(receipt_status) NOT LIKE 'sent%'
                            AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                        THEN 1
                        ELSE 0
                    END), 0) AS in_flight_count
                 FROM messages",
            )?;
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
        })?;
        Ok((queued.max(0) as u64, in_flight.max(0) as u64))
    }

    pub fn count_outbound_messages(&self) -> rusqlite::Result<u64> {
        let count: i64 = self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE direction = 'out'",
                [],
                |row| row.get(0),
            )
        })?;
        Ok(count.max(0) as u64)
    }

    pub fn expire_outbound_messages_before(&self, cutoff_ts: i64) -> rusqlite::Result<Vec<String>> {
        self.with_write_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id
                 FROM messages
                 WHERE direction = 'out'
                   AND timestamp < ?1
                   AND (
                        receipt_status IS NULL
                        OR TRIM(receipt_status) = ''
                        OR (
                            LOWER(receipt_status) NOT LIKE 'sent%'
                            AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                        )
                   )
                 ORDER BY timestamp ASC, id ASC",
            )?;
            let mut rows = stmt.query(params![cutoff_ts])?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(row.get::<_, String>(0)?);
            }
            drop(rows);
            drop(stmt);
            for message_id in ids.iter() {
                conn.execute(
                    "UPDATE messages SET receipt_status = 'expired' WHERE id = ?1",
                    params![message_id],
                )?;
            }
            Ok(ids)
        })
    }

    pub fn prune_outbound_messages(
        &self,
        count: usize,
        eviction_priority: &str,
    ) -> rusqlite::Result<Vec<String>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        self.with_write_conn(|conn| {
            let collect_ids = |query: &str, remaining: usize| -> rusqlite::Result<Vec<String>> {
                if remaining == 0 {
                    return Ok(Vec::new());
                }
                let mut stmt = conn.prepare(query)?;
                let mut rows = stmt.query(params![remaining as i64])?;
                let mut ids = Vec::new();
                while let Some(row) = rows.next()? {
                    ids.push(row.get::<_, String>(0)?);
                }
                Ok(ids)
            };

            let normalized = eviction_priority.trim().to_ascii_lowercase();
            let mut ids = if normalized == "terminal_first" {
                let mut selected = collect_ids(
                    "SELECT id
                     FROM messages
                     WHERE direction = 'out'
                       AND receipt_status IS NOT NULL
                       AND TRIM(receipt_status) <> ''
                       AND (
                            LOWER(receipt_status) LIKE 'sent%'
                            OR LOWER(receipt_status) IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                       )
                     ORDER BY timestamp ASC, id ASC
                     LIMIT ?1",
                    count,
                )?;
                let remaining = count.saturating_sub(selected.len());
                if remaining > 0 {
                    let mut non_terminal = collect_ids(
                        "SELECT id
                         FROM messages
                         WHERE direction = 'out'
                           AND (
                                receipt_status IS NULL
                                OR TRIM(receipt_status) = ''
                                OR (
                                    LOWER(receipt_status) NOT LIKE 'sent%'
                                    AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                                )
                           )
                         ORDER BY timestamp ASC, id ASC
                         LIMIT ?1",
                        remaining,
                    )?;
                    selected.append(&mut non_terminal);
                }
                selected
            } else {
                collect_ids(
                    "SELECT id
                     FROM messages
                     WHERE direction = 'out'
                     ORDER BY timestamp ASC, id ASC
                     LIMIT ?1",
                    count,
                )?
            };

            ids.sort();
            ids.dedup();
            for message_id in ids.iter() {
                conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
            }
            if !ids.is_empty() {
                self.write_state
                    .message_count_cache
                    .fetch_sub(ids.len().min(u64::MAX as usize) as u64, Ordering::Relaxed);
            }
            Ok(ids)
        })
    }

    pub fn prune_messages_to_limit_bytes(&self, limit_bytes: u64) -> rusqlite::Result<Vec<String>> {
        self.with_write_conn(|conn| {
            let current_bytes: i64 = conn.query_row(
                "SELECT COALESCE(SUM(
                    LENGTH(id) +
                    LENGTH(source) +
                    LENGTH(destination) +
                    LENGTH(title) +
                    LENGTH(content) +
                    LENGTH(direction) +
                    COALESCE(LENGTH(fields), 0) +
                    COALESCE(LENGTH(receipt_status), 0)
                ), 0) FROM messages",
                [],
                |row| row.get(0),
            )?;
            if current_bytes.max(0) as u64 <= limit_bytes {
                return Ok(Vec::new());
            }

            let mut stmt = conn.prepare(
                "SELECT id,
                        LENGTH(id) +
                        LENGTH(source) +
                        LENGTH(destination) +
                        LENGTH(title) +
                        LENGTH(content) +
                        LENGTH(direction) +
                        COALESCE(LENGTH(fields), 0) +
                        COALESCE(LENGTH(receipt_status), 0) AS approx_bytes
                 FROM messages
                 ORDER BY timestamp ASC, id ASC",
            )?;
            let mut rows = stmt.query([])?;
            let mut bytes = current_bytes.max(0) as u64;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                if bytes <= limit_bytes {
                    break;
                }
                let id: String = row.get(0)?;
                let approx_bytes: i64 = row.get(1)?;
                ids.push(id);
                bytes = bytes.saturating_sub(approx_bytes.max(0) as u64);
            }
            drop(rows);
            drop(stmt);

            for message_id in ids.iter() {
                conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
            }
            if !ids.is_empty() {
                self.write_state
                    .message_count_cache
                    .fetch_sub(ids.len().min(u64::MAX as usize) as u64, Ordering::Relaxed);
            }
            Ok(ids)
        })
    }

    pub fn update_receipt_status(&self, message_id: &str, status: &str) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute(
                "UPDATE messages SET receipt_status = ?1 WHERE id = ?2",
                params![status, message_id],
            )?;
            Ok(())
        })
    }

    pub fn resolve_receipt_status(
        &self,
        message_id: &str,
        candidate_status: &str,
    ) -> rusqlite::Result<Option<String>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::ResolveReceiptStatus {
                message_id: message_id.to_string(),
                candidate_status: candidate_status.to_string(),
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn clear_messages(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute("DELETE FROM messages", [])?;
            self.write_state.message_count_cache.store(0, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn insert_announce(&self, record: &AnnounceRecord) -> rusqlite::Result<()> {
        let capabilities_json = serde_json::to_string(&record.capabilities).unwrap_or_default();
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO announces (id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    &record.id,
                    &record.peer,
                    record.timestamp,
                    &record.name,
                    &record.name_source,
                    record.first_seen,
                    record.seen_count as i64,
                    &record.app_data_hex,
                    capabilities_json,
                    record.rssi,
                    record.snr,
                    record.q,
                    record.stamp_cost_flexibility,
                    record.peering_cost,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_announces(
        &self,
        limit: usize,
        before_ts: Option<i64>,
        before_id: Option<&str>,
    ) -> rusqlite::Result<Vec<AnnounceRecord>> {
        self.with_read_conn(|conn| {
            let mut records = Vec::new();
            let parse_row = |row: &rusqlite::Row| -> rusqlite::Result<AnnounceRecord> {
                let capabilities_json: Option<String> = row.get(8)?;
                let capabilities = capabilities_json
                    .as_deref()
                    .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
                    .unwrap_or_default();
                let seen_count: i64 = row.get(6)?;
                Ok(AnnounceRecord {
                    id: row.get(0)?,
                    peer: row.get(1)?,
                    timestamp: row.get(2)?,
                    name: row.get(3)?,
                    name_source: row.get(4)?,
                    first_seen: row.get(5)?,
                    seen_count: seen_count.max(0) as u64,
                    app_data_hex: row.get(7)?,
                    capabilities,
                    rssi: row.get(9)?,
                    snr: row.get(10)?,
                    q: row.get(11)?,
                    stamp_cost_flexibility: row.get(12)?,
                    peering_cost: row.get(13)?,
                })
            };
            if let Some(ts) = before_ts {
                let query_with_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost FROM announces WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3";
                let query_without_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost FROM announces WHERE timestamp < ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2";
                if let Some(ann_id) = before_id {
                    let mut stmt = conn.prepare(query_with_id)?;
                    let mut rows = stmt.query(params![ts, ann_id, limit as i64])?;
                    while let Some(row) = rows.next()? {
                        records.push(parse_row(row)?);
                    }
                } else {
                    let mut stmt = conn.prepare(query_without_id)?;
                    let mut rows = stmt.query(params![ts, limit as i64])?;
                    while let Some(row) = rows.next()? {
                        records.push(parse_row(row)?);
                    }
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost FROM announces ORDER BY timestamp DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(parse_row(row)?);
                }
            }
            Ok(records)
        })
    }

    pub fn clear_announces(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute("DELETE FROM announces", [])?;
            Ok(())
        })
    }

    pub fn put_sdk_domain_snapshot(&self, snapshot: &JsonValue) -> rusqlite::Result<()> {
        let snapshot_json = serde_json::to_string(snapshot)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO sdk_domain_state (domain, state_json) VALUES (?1, ?2)
                 ON CONFLICT(domain) DO UPDATE SET state_json = excluded.state_json",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY, snapshot_json],
            )?;
            Ok(())
        })
    }

    pub fn get_sdk_domain_snapshot(&self) -> rusqlite::Result<Option<JsonValue>> {
        let snapshot_json: Option<String> = self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT state_json FROM sdk_domain_state WHERE domain = ?1 LIMIT 1",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY],
                |row| row.get(0),
            )
            .optional()
        })?;
        let Some(snapshot_json) = snapshot_json else {
            return Ok(None);
        };
        let parsed = serde_json::from_str(snapshot_json.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(Some(parsed))
    }

    pub fn clear_sdk_domain_snapshot(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM sdk_domain_state WHERE domain = ?1",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY],
            )?;
            Ok(())
        })
    }

    fn configure_connection(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
            conn.pragma_update(None, "busy_timeout", 5_000i64)?;
            Ok(())
        })?;
        if self.read_conn.is_some() {
            self.with_read_conn(|conn| {
                conn.pragma_update(None, "busy_timeout", 5_000i64)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn busy_timeout_ms(&self) -> rusqlite::Result<i64> {
        self.with_write_conn(|conn| conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0)))
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    destination TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    timestamp INTEGER NOT NULL,
                    direction TEXT NOT NULL,
                    fields TEXT,
                    receipt_status TEXT
                );
                CREATE TABLE IF NOT EXISTS announces (
                    id TEXT PRIMARY KEY,
                    peer TEXT NOT NULL,
                    timestamp INTEGER NOT NULL,
                    name TEXT,
                    name_source TEXT,
                    first_seen INTEGER NOT NULL,
                    seen_count INTEGER NOT NULL,
                    app_data_hex TEXT,
                    capabilities TEXT,
                    rssi REAL,
                    snr REAL,
                    q REAL,
                    stamp_cost_flexibility INTEGER,
                    peering_cost INTEGER
                );
                CREATE TABLE IF NOT EXISTS sdk_domain_state (
                    domain TEXT PRIMARY KEY,
                    state_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_messages_timestamp_desc
                    ON messages(timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_messages_direction_timestamp_desc
                    ON messages(direction, timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_messages_receipt_status
                    ON messages(receipt_status);
                CREATE INDEX IF NOT EXISTS idx_announces_timestamp_id_desc
                    ON announces(timestamp DESC, id DESC);",
            )?;
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN title TEXT", []);
            let _ = conn.execute("UPDATE messages SET title = '' WHERE title IS NULL", []);
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN fields TEXT", []);
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN receipt_status TEXT", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN name TEXT", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN name_source TEXT", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN first_seen INTEGER", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN seen_count INTEGER", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN app_data_hex TEXT", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN capabilities TEXT", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN rssi REAL", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN snr REAL", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN q REAL", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN stamp_cost_flexibility INTEGER", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN peering_cost INTEGER", []);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outbound_message(id: &str, timestamp: i64, receipt_status: Option<&str>) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp,
            direction: "out".to_string(),
            fields: None,
            receipt_status: receipt_status.map(ToString::to_string),
        }
    }

    #[test]
    fn sdk_domain_snapshot_roundtrip() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let initial = store.get_sdk_domain_snapshot().expect("query snapshot");
        assert!(initial.is_none(), "snapshot should be absent before first write");

        let snapshot = json!({
            "topics": [{ "topic_id": "topic-1" }],
            "attachments": [],
            "markers": [],
        });
        store.put_sdk_domain_snapshot(&snapshot).expect("persist snapshot");

        let loaded = store.get_sdk_domain_snapshot().expect("load snapshot");
        assert_eq!(loaded, Some(snapshot));
    }

    #[test]
    fn sdk_domain_snapshot_clear_removes_record() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .put_sdk_domain_snapshot(&json!({ "voice_sessions": [{ "session_id": "voice-1" }] }))
            .expect("persist snapshot");
        store.clear_sdk_domain_snapshot().expect("clear snapshot");
        let loaded = store.get_sdk_domain_snapshot().expect("load snapshot");
        assert!(loaded.is_none(), "snapshot should be removed after clear");
    }

    #[test]
    fn message_count_uses_direct_count() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, None))
            .expect("insert msg-1");
        store
            .insert_message(&outbound_message("msg-2", 2, Some("delivered")))
            .expect("insert msg-2");

        assert_eq!(store.message_count().expect("count messages"), 2);
    }

    #[test]
    fn message_count_cache_ignores_replace_for_existing_id() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, None))
            .expect("insert original");
        store
            .insert_message(&outbound_message("msg-1", 2, Some("delivered")))
            .expect("replace existing");

        assert_eq!(store.message_count().expect("count messages"), 1);
    }

    #[test]
    fn configure_connection_sets_busy_timeout() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let busy_timeout_ms = store.busy_timeout_ms().expect("query busy_timeout");
        assert_eq!(busy_timeout_ms, 5_000);
    }

    #[test]
    fn expire_outbound_messages_marks_non_terminal_records() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("out-non-terminal", 10, None))
            .expect("insert non-terminal");
        store
            .insert_message(&outbound_message("out-terminal", 10, Some("delivered")))
            .expect("insert terminal");
        let expired = store.expire_outbound_messages_before(11).expect("expire outbound");
        assert_eq!(expired, vec!["out-non-terminal".to_string()]);
        let non_terminal = store
            .get_message("out-non-terminal")
            .expect("load non-terminal")
            .expect("non-terminal exists");
        assert_eq!(non_terminal.receipt_status.as_deref(), Some("expired"));
        let terminal =
            store.get_message("out-terminal").expect("load terminal").expect("terminal exists");
        assert_eq!(terminal.receipt_status.as_deref(), Some("delivered"));
    }

    #[test]
    fn prune_outbound_messages_terminal_first_prefers_terminal_records() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-terminal-old", 1, Some("sent: direct")))
            .expect("insert terminal old");
        store
            .insert_message(&outbound_message("msg-non-terminal", 2, None))
            .expect("insert non-terminal");
        store
            .insert_message(&outbound_message("msg-terminal-new", 3, Some("delivered")))
            .expect("insert terminal new");

        let pruned = store.prune_outbound_messages(2, "terminal_first").expect("prune outbound");
        assert_eq!(pruned.len(), 2);
        assert!(pruned.iter().any(|id| id == "msg-terminal-old"));
        assert!(pruned.iter().any(|id| id == "msg-terminal-new"));
        assert!(
            store.get_message("msg-non-terminal").expect("load non-terminal").is_some(),
            "non-terminal record should remain when terminal records satisfy prune count"
        );
        assert_eq!(store.message_count().expect("count after prune"), 1);
    }

    #[test]
    fn clear_messages_resets_message_count_cache() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, None))
            .expect("insert msg-1");
        store
            .insert_message(&outbound_message("msg-2", 2, Some("delivered")))
            .expect("insert msg-2");

        store.clear_messages().expect("clear messages");

        assert_eq!(store.message_count().expect("count after clear"), 0);
    }

    #[test]
    fn prune_messages_to_limit_bytes_removes_oldest_messages() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut first = outbound_message("msg-1", 1, None);
        first.content = "a".repeat(128);
        let mut second = outbound_message("msg-2", 2, None);
        second.content = "b".repeat(128);
        store.insert_message(&first).expect("insert first");
        store.insert_message(&second).expect("insert second");

        let before = store.message_storage_stats().expect("stats before");
        let pruned = store
            .prune_messages_to_limit_bytes(before.bytes.saturating_sub(64))
            .expect("prune");

        assert_eq!(pruned, vec!["msg-1".to_string()]);
        let remaining = store.list_messages(10, None).expect("remaining");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "msg-2");
    }

    #[test]
    fn peer_message_stats_reports_incoming_and_outgoing_counts() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut outbound = outbound_message("msg-out", 1, None);
        outbound.destination = "peer-a".to_string();
        let inbound = MessageRecord {
            id: "msg-in".to_string(),
            source: "peer-a".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 2,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        };
        store.insert_message(&outbound).expect("insert outbound");
        store.insert_message(&inbound).expect("insert inbound");

        let stats = store.peer_message_stats("peer-a").expect("peer stats");
        assert_eq!(stats.outgoing, 1);
        assert_eq!(stats.incoming, 1);
        assert_eq!(stats.offered, 1);
        assert_eq!(stats.unhandled, 1);
    }

    #[test]
    fn resolve_receipt_status_updates_non_terminal_message() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, None))
            .expect("insert message");

        let resolved = store
            .resolve_receipt_status("msg-1", "sent: direct")
            .expect("resolve status");

        assert_eq!(resolved.as_deref(), Some("sent: direct"));
        assert_eq!(
            store
                .get_message("msg-1")
                .expect("load message")
                .expect("message exists")
                .receipt_status
                .as_deref(),
            Some("sent: direct")
        );
    }

    #[test]
    fn resolve_receipt_status_preserves_terminal_status() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("delivered")))
            .expect("insert delivered message");

        let resolved = store
            .resolve_receipt_status("msg-1", "sent: direct")
            .expect("resolve status");

        assert_eq!(resolved.as_deref(), Some("delivered"));
        assert_eq!(
            store
                .get_message("msg-1")
                .expect("load message")
                .expect("message exists")
                .receipt_status
                .as_deref(),
            Some("delivered")
        );
    }
}
