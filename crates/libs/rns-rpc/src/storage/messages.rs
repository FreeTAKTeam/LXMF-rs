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

impl MessagesStore {
    const SDK_DOMAIN_SNAPSHOT_KEY: &'static str = "sdk_domains.v1";

    fn is_terminal_receipt_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("failed")
            || matches!(normalized.as_str(), "cancelled" | "delivered" | "expired" | "rejected")
    }

    fn should_preserve_receipt_status(existing_status: &str, candidate_status: &str) -> bool {
        if Self::is_terminal_receipt_status(existing_status) {
            return true;
        }

        let existing = existing_status.trim().to_ascii_lowercase();
        let candidate = candidate_status.trim().to_ascii_lowercase();
        existing.starts_with("sent") && candidate.starts_with("sending")
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
        self.write_state.message_count_cache.store(count.max(0) as u64, Ordering::Relaxed);
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
                            let _ = reply
                                .send(Self::insert_message_direct(write_state.as_ref(), &record));
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
                        OutboundWriteCommand::PruneMessagesToLimitBytes { limit_bytes, reply } => {
                            let result = Self::prune_messages_to_limit_bytes_direct(
                                write_state.as_ref(),
                                limit_bytes,
                            );
                            if let Some(reply) = reply {
                                let _ = reply.send(result);
                            }
                        }
                        OutboundWriteCommand::UpdateReceiptStatus { message_id, status, reply } => {
                            let _ = reply.send(Self::update_receipt_status_direct(
                                write_state.as_ref(),
                                message_id.as_str(),
                                status.as_str(),
                            ));
                        }
                        OutboundWriteCommand::UpdateMessageFields {
                            message_id,
                            fields_json,
                            reply,
                        } => {
                            let _ = reply.send(Self::update_message_fields_direct(
                                write_state.as_ref(),
                                message_id.as_str(),
                                fields_json.as_deref(),
                            ));
                        }
                        OutboundWriteCommand::InsertAnnounce { record, reply } => {
                            let _ = reply
                                .send(Self::insert_announce_direct(write_state.as_ref(), &record));
                        }
                        OutboundWriteCommand::UpsertTicket {
                            destination,
                            ticket,
                            expires_at,
                            reply,
                        } => {
                            let _ = reply.send(Self::upsert_ticket_direct(
                                write_state.as_ref(),
                                destination.as_str(),
                                ticket.as_str(),
                                expires_at,
                            ));
                        }
                        OutboundWriteCommand::PruneExpiredTickets {
                            now,
                            inbound_grace_secs,
                            reply,
                        } => {
                            let _ = reply.send(Self::prune_expired_tickets_direct(
                                write_state.as_ref(),
                                now,
                                inbound_grace_secs,
                            ));
                        }
                        OutboundWriteCommand::UpsertOutboundTicket {
                            destination,
                            ticket,
                            expires_at,
                            reply,
                        } => {
                            let _ = reply.send(Self::upsert_outbound_ticket_direct(
                                write_state.as_ref(),
                                destination.as_str(),
                                ticket.as_str(),
                                expires_at,
                            ));
                        }
                        OutboundWriteCommand::UpsertTicketLastDelivery {
                            destination,
                            delivered_at,
                            reply,
                        } => {
                            let _ = reply.send(Self::upsert_ticket_last_delivery_direct(
                                write_state.as_ref(),
                                destination.as_str(),
                                delivered_at,
                            ));
                        }
                    }
                }
            })
            .expect("spawn messages outbound writer");
    }

    fn with_write_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let started = std::time::Instant::now();
        let conn = self.write_state.conn.lock().expect("messages sqlite write mutex poisoned");
        let waited_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.write_state.write_lock_wait_ns_total.fetch_add(waited_ns, Ordering::Relaxed);
        self.write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
        f(&conn)
    }

    fn with_read_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
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
        write_state.write_lock_wait_ns_total.fetch_add(waited_ns, Ordering::Relaxed);
        write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
        f(&conn)
    }

    fn insert_message_direct(
        write_state: &WriteState,
        record: &MessageRecord,
    ) -> rusqlite::Result<()> {
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
                if Self::should_preserve_receipt_status(existing_status.as_str(), candidate_status)
                {
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

    fn update_receipt_status_direct(
        write_state: &WriteState,
        message_id: &str,
        status: &str,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "UPDATE messages SET receipt_status = ?1 WHERE id = ?2",
                params![status, message_id],
            )?;
            Ok(())
        })
    }

    fn update_message_fields_direct(
        write_state: &WriteState,
        message_id: &str,
        fields_json: Option<&str>,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "UPDATE messages SET fields = ?1 WHERE id = ?2",
                params![fields_json, message_id],
            )?;
            Ok(())
        })
    }

    fn insert_announce_direct(
        write_state: &WriteState,
        record: &AnnounceRecord,
    ) -> rusqlite::Result<()> {
        let capabilities_json = serde_json::to_string(&record.capabilities).unwrap_or_default();
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO announces (id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
                    record.stamp_cost,
                    record.stamp_cost_flexibility,
                    record.peering_cost,
                ],
            )?;
            Ok(())
        })
    }

    fn upsert_ticket_direct(
        write_state: &WriteState,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "INSERT INTO tickets (destination, ticket, expires_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(destination, ticket) DO UPDATE SET expires_at = excluded.expires_at",
                params![destination, ticket, expires_at],
            )?;
            Ok(())
        })
    }

    fn prune_expired_tickets_direct(
        write_state: &WriteState,
        now: i64,
        inbound_grace_secs: i64,
    ) -> rusqlite::Result<()> {
        let inbound_cutoff = now.saturating_sub(inbound_grace_secs.max(0));
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute("DELETE FROM outbound_tickets WHERE expires_at <= ?1", params![now])?;
            conn.execute("DELETE FROM tickets WHERE expires_at < ?1", params![inbound_cutoff])?;
            Ok(())
        })
    }

    fn upsert_outbound_ticket_direct(
        write_state: &WriteState,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "INSERT INTO outbound_tickets (destination, ticket, expires_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(destination) DO UPDATE SET ticket = excluded.ticket, expires_at = excluded.expires_at",
                params![destination, ticket, expires_at],
            )?;
            Ok(())
        })
    }

    fn upsert_ticket_last_delivery_direct(
        write_state: &WriteState,
        destination: &str,
        delivered_at: i64,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "INSERT INTO ticket_deliveries (destination, delivered_at) VALUES (?1, ?2)
                 ON CONFLICT(destination) DO UPDATE SET delivered_at = excluded.delivered_at",
                params![destination, delivered_at],
            )?;
            Ok(())
        })
    }

    pub fn insert_message(&self, record: &MessageRecord) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::InsertMessage { record: record.clone(), reply: reply_tx })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn list_messages(
        &self,
        limit: usize,
        before_ts: Option<i64>,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        self.list_messages_page(limit, before_ts, None)
    }

    pub fn list_messages_page(
        &self,
        limit: usize,
        before_ts: Option<i64>,
        before_id: Option<&str>,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        self.with_read_conn(|conn| {
            let mut records = Vec::new();
            if let Some(ts) = before_ts {
                let mut stmt = if before_id.is_some() {
                    conn.prepare(
                        "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3",
                    )?
                } else {
                    conn.prepare(
                        "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE timestamp < ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2",
                    )?
                };
                let mut rows = if let Some(before_id) = before_id {
                    stmt.query(params![ts, before_id, limit as i64])?
                } else {
                    stmt.query(params![ts, limit as i64])?
                };
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
                    "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages ORDER BY timestamp DESC, id DESC LIMIT ?1",
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
            Ok(MessageStorageStats { count, bytes: bytes.unwrap_or(0).max(0) as u64 })
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
                                AND LOWER(receipt_status) NOT LIKE 'failed%'
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
                            AND LOWER(receipt_status) NOT LIKE 'failed%'
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
            conn.query_row("SELECT COUNT(*) FROM messages WHERE direction = 'out'", [], |row| {
                row.get(0)
            })
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
                            AND LOWER(receipt_status) NOT LIKE 'failed%'
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
                            OR LOWER(receipt_status) LIKE 'failed%'
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
                                    AND LOWER(receipt_status) NOT LIKE 'failed%'
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

    fn prune_messages_to_limit_bytes_direct(
        write_state: &WriteState,
        limit_bytes: u64,
    ) -> rusqlite::Result<Vec<String>> {
        Self::write_lock_and_run(write_state, |conn| {
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
                write_state
                    .message_count_cache
                    .fetch_sub(ids.len().min(u64::MAX as usize) as u64, Ordering::Relaxed);
            }
            Ok(ids)
        })
    }

    pub fn prune_messages_to_limit_bytes(&self, limit_bytes: u64) -> rusqlite::Result<Vec<String>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::PruneMessagesToLimitBytes {
                limit_bytes,
                reply: Some(reply_tx),
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn schedule_prune_messages_to_limit_bytes(&self, limit_bytes: u64) -> rusqlite::Result<()> {
        self.outbound_write_tx
            .send(OutboundWriteCommand::PruneMessagesToLimitBytes { limit_bytes, reply: None })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
    }

    pub fn update_receipt_status(&self, message_id: &str, status: &str) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpdateReceiptStatus {
                message_id: message_id.to_string(),
                status: status.to_string(),
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn update_message_fields(
        &self,
        message_id: &str,
        fields: Option<&JsonValue>,
    ) -> rusqlite::Result<()> {
        let fields_json = fields
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpdateMessageFields {
                message_id: message_id.to_string(),
                fields_json,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
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
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::InsertAnnounce { record: record.clone(), reply: reply_tx })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
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
                    stamp_cost: row.get(12)?,
                    stamp_cost_flexibility: row.get(13)?,
                    peering_cost: row.get(14)?,
                })
            };
            if let Some(ts) = before_ts {
                let query_with_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3";
                let query_without_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces WHERE timestamp < ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2";
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
                    "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces ORDER BY timestamp DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(parse_row(row)?);
                }
            }
            Ok(records)
        })
    }

    pub fn latest_announce_stamp_cost_for(&self, peer: &str) -> rusqlite::Result<Option<u32>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT stamp_cost FROM announces WHERE peer = ?1 AND stamp_cost IS NOT NULL ORDER BY timestamp DESC, id DESC LIMIT 1",
                params![peer],
                |row| row.get(0),
            )
            .optional()
        })
    }

    pub fn get_ticket(&self, destination: &str) -> rusqlite::Result<Option<(String, i64)>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT ticket, expires_at FROM tickets WHERE destination = ?1 ORDER BY expires_at DESC, ticket DESC LIMIT 1",
                params![destination],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    pub fn get_tickets_for_destination(
        &self,
        destination: &str,
    ) -> rusqlite::Result<Vec<(String, i64)>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT ticket, expires_at FROM tickets WHERE destination = ?1 ORDER BY expires_at DESC, ticket DESC",
            )?;
            let rows = stmt.query_map(params![destination], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect()
        })
    }

    pub fn upsert_ticket(
        &self,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertTicket {
                destination: destination.to_string(),
                ticket: ticket.to_string(),
                expires_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn prune_expired_tickets(&self, now: i64, inbound_grace_secs: i64) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::PruneExpiredTickets {
                now,
                inbound_grace_secs,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn get_outbound_ticket(
        &self,
        destination: &str,
    ) -> rusqlite::Result<Option<(String, i64)>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT ticket, expires_at FROM outbound_tickets WHERE destination = ?1",
                params![destination],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    pub fn upsert_outbound_ticket(
        &self,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertOutboundTicket {
                destination: destination.to_string(),
                ticket: ticket.to_string(),
                expires_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn get_ticket_last_delivery(&self, destination: &str) -> rusqlite::Result<Option<i64>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT delivered_at FROM ticket_deliveries WHERE destination = ?1",
                params![destination],
                |row| row.get(0),
            )
            .optional()
        })
    }

    pub fn upsert_ticket_last_delivery(
        &self,
        destination: &str,
        delivered_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertTicketLastDelivery {
                destination: destination.to_string(),
                delivered_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
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
                    stamp_cost INTEGER,
                    stamp_cost_flexibility INTEGER,
                    peering_cost INTEGER
                );
                CREATE TABLE IF NOT EXISTS sdk_domain_state (
                    domain TEXT PRIMARY KEY,
                    state_json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS tickets (
                    destination TEXT NOT NULL,
                    ticket TEXT NOT NULL,
                    expires_at INTEGER NOT NULL,
                    PRIMARY KEY(destination, ticket)
                );
                CREATE TABLE IF NOT EXISTS outbound_tickets (
                    destination TEXT PRIMARY KEY,
                    ticket TEXT NOT NULL,
                    expires_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS ticket_deliveries (
                    destination TEXT PRIMARY KEY,
                    delivered_at INTEGER NOT NULL
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
            Self::ensure_multi_ticket_schema(conn)?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_tickets_destination_expires
                    ON tickets(destination, expires_at DESC)",
                [],
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
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN stamp_cost INTEGER", []);
            let _ =
                conn.execute("ALTER TABLE announces ADD COLUMN stamp_cost_flexibility INTEGER", []);
            let _ = conn.execute("ALTER TABLE announces ADD COLUMN peering_cost INTEGER", []);
            Ok(())
        })
    }

    fn ensure_multi_ticket_schema(conn: &Connection) -> rusqlite::Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(tickets)")?;
        let mut rows = stmt.query([])?;
        let mut primary_key_columns = Vec::new();
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            let pk_order: i64 = row.get(5)?;
            if pk_order > 0 {
                primary_key_columns.push((pk_order, name));
            }
        }
        primary_key_columns.sort_by_key(|(pk_order, _)| *pk_order);
        let primary_key_columns: Vec<String> =
            primary_key_columns.into_iter().map(|(_, name)| name).collect();

        if primary_key_columns != ["destination"] {
            return Ok(());
        }

        conn.execute_batch(
            "ALTER TABLE tickets RENAME TO tickets_single_destination;
             CREATE TABLE tickets (
                destination TEXT NOT NULL,
                ticket TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                PRIMARY KEY(destination, ticket)
             );
             INSERT OR IGNORE INTO tickets (destination, ticket, expires_at)
                SELECT destination, ticket, expires_at FROM tickets_single_destination;
             DROP TABLE tickets_single_destination;",
        )
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
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert msg-1");
        store
            .insert_message(&outbound_message("msg-2", 2, Some("delivered")))
            .expect("insert msg-2");

        assert_eq!(store.message_count().expect("count messages"), 2);
    }

    #[test]
    fn message_count_cache_ignores_replace_for_existing_id() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert original");
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
    fn prune_expired_tickets_matches_python_available_ticket_cleanup() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.upsert_outbound_ticket("expired-outbound", "00", 90).expect("outbound expired");
        store.upsert_outbound_ticket("valid-outbound", "11", 110).expect("outbound valid");
        store.upsert_ticket("inbound-grace", "22", 90).expect("inbound grace");
        store.upsert_ticket("inbound-expired", "33", 89).expect("inbound expired");

        store.prune_expired_tickets(100, 10).expect("prune tickets");

        assert!(store.get_outbound_ticket("expired-outbound").expect("expired outbound").is_none());
        assert!(store.get_outbound_ticket("valid-outbound").expect("valid outbound").is_some());
        assert!(store.get_ticket("inbound-grace").expect("inbound grace").is_some());
        assert!(store.get_ticket("inbound-expired").expect("inbound expired").is_none());
    }

    #[test]
    fn announce_and_ticket_writes_run_on_writer_lane() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_announce(&AnnounceRecord {
                id: "ann-1".to_string(),
                peer: "peer-a".to_string(),
                timestamp: 100,
                name: Some("Peer A".to_string()),
                name_source: Some("app_data".to_string()),
                first_seen: 90,
                seen_count: 2,
                app_data_hex: Some("0102".to_string()),
                capabilities: vec!["lxmf.delivery".to_string()],
                rssi: Some(-42.0),
                snr: Some(7.0),
                q: Some(0.9),
                stamp_cost: Some(4),
                stamp_cost_flexibility: Some(1),
                peering_cost: Some(2),
            })
            .expect("insert announce");
        store.upsert_ticket("peer-a", "22", 200).expect("upsert inbound ticket");
        store.upsert_outbound_ticket("peer-a", "33", 210).expect("upsert outbound ticket");
        store.upsert_ticket_last_delivery("peer-a", 110).expect("upsert last delivery");

        let announces = store.list_announces(10, None, None).expect("list announces");
        assert_eq!(announces.len(), 1);
        assert_eq!(announces[0].peer, "peer-a");
        assert_eq!(announces[0].capabilities, vec!["lxmf.delivery".to_string()]);
        assert_eq!(store.get_ticket("peer-a").expect("inbound ticket"), Some(("22".into(), 200)));
        assert_eq!(
            store.get_outbound_ticket("peer-a").expect("outbound ticket"),
            Some(("33".into(), 210))
        );
        assert_eq!(store.get_ticket_last_delivery("peer-a").expect("last delivery"), Some(110));
    }

    #[test]
    fn inbound_tickets_keep_multiple_generated_tickets_per_destination() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.upsert_ticket("peer", "22", 200).expect("insert first ticket");
        store.upsert_ticket("peer", "33", 300).expect("insert second ticket");

        let tickets = store.get_tickets_for_destination("peer").expect("load tickets");

        assert_eq!(tickets, vec![("33".to_string(), 300), ("22".to_string(), 200)]);
        assert_eq!(store.get_ticket("peer").expect("load latest"), Some(("33".to_string(), 300)));
    }

    #[test]
    fn opening_old_single_ticket_schema_migrates_to_multi_ticket_schema() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("single-ticket-schema.sqlite");
        {
            let conn = Connection::open(db_path.as_path()).expect("open raw sqlite");
            conn.execute_batch(
                "CREATE TABLE tickets (
                    destination TEXT PRIMARY KEY,
                    ticket TEXT NOT NULL,
                    expires_at INTEGER NOT NULL
                );
                INSERT INTO tickets (destination, ticket, expires_at)
                    VALUES ('peer', '22', 200);",
            )
            .expect("seed old schema");
        }

        let store = MessagesStore::open(db_path.as_path()).expect("open migrated store");
        store.upsert_ticket("peer", "33", 300).expect("insert second ticket");

        let tickets = store.get_tickets_for_destination("peer").expect("load tickets");
        assert_eq!(tickets, vec![("33".to_string(), 300), ("22".to_string(), 200)]);
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
    fn detailed_failed_status_is_terminal_for_expiry_and_buckets() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("out-failed-detail", 10, Some("failed: no path")))
            .expect("insert detailed failure");
        store
            .insert_message(&outbound_message("out-sending", 11, Some("sending")))
            .expect("insert sending");

        let expired = store.expire_outbound_messages_before(12).expect("expire outbound");
        assert_eq!(expired, vec!["out-sending".to_string()]);
        let failed =
            store.get_message("out-failed-detail").expect("load failed").expect("failed exists");
        assert_eq!(failed.receipt_status.as_deref(), Some("failed: no path"));

        let (queued, in_flight) = store.count_message_buckets().expect("message buckets");
        assert_eq!(queued, 0);
        assert_eq!(in_flight, 0);
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
    fn prune_outbound_messages_terminal_first_treats_detailed_failed_status_as_terminal() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-failed-detail", 1, Some("failed: no path")))
            .expect("insert detailed failure");
        store
            .insert_message(&outbound_message("msg-sending", 2, Some("sending")))
            .expect("insert sending");

        let pruned = store.prune_outbound_messages(1, "terminal_first").expect("prune outbound");
        assert_eq!(pruned, vec!["msg-failed-detail".to_string()]);
        assert!(
            store.get_message("msg-sending").expect("load sending").is_some(),
            "sending record should remain when detailed failure satisfies prune count"
        );
    }

    #[test]
    fn peer_message_stats_treats_detailed_failed_status_as_terminal() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut failed = outbound_message("peer-failed-detail", 1, Some("failed: no path"));
        failed.destination = "peer-a".to_string();
        let mut sending = outbound_message("peer-sending", 2, Some("sending"));
        sending.destination = "peer-a".to_string();
        store.insert_message(&failed).expect("insert detailed failure");
        store.insert_message(&sending).expect("insert sending");

        let stats = store.peer_message_stats("peer-a").expect("peer stats");
        assert_eq!(stats.outgoing, 2);
        assert_eq!(stats.offered, 1);
    }

    #[test]
    fn clear_messages_resets_message_count_cache() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert msg-1");
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
        let pruned =
            store.prune_messages_to_limit_bytes(before.bytes.saturating_sub(64)).expect("prune");

        assert_eq!(pruned, vec!["msg-1".to_string()]);
        let remaining = store.list_messages(10, None).expect("remaining");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "msg-2");
    }

    #[test]
    fn scheduled_prune_messages_to_limit_bytes_runs_on_writer_lane() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let mut first = outbound_message("msg-1", 1, None);
        first.content = "a".repeat(128);
        let mut second = outbound_message("msg-2", 2, None);
        second.content = "b".repeat(128);
        store.insert_message(&first).expect("insert first");
        store.insert_message(&second).expect("insert second");

        let before = store.message_storage_stats().expect("stats before");
        store
            .schedule_prune_messages_to_limit_bytes(before.bytes.saturating_sub(64))
            .expect("schedule prune");
        store
            .insert_message(&outbound_message("flush-after-scheduled-prune", 3, Some("sent")))
            .expect("flush writer lane");

        let remaining = store.list_messages(10, None).expect("remaining");
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|record| record.id == "msg-2"));
        assert!(remaining.iter().any(|record| record.id == "flush-after-scheduled-prune"));
        assert!(
            remaining.iter().all(|record| record.id != "msg-1"),
            "scheduled prune should remove the oldest oversized record before later writes"
        );
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
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert message");

        let resolved =
            store.resolve_receipt_status("msg-1", "sent: direct").expect("resolve status");

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
    fn update_message_fields_preserves_receipt_status() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("sending")))
            .expect("insert message");

        store
            .update_message_fields("msg-1", Some(&json!({"_lxmf": {"transient_id": "abcd"}})))
            .expect("update fields");

        let message = store.get_message("msg-1").expect("load message").expect("message exists");
        assert_eq!(message.receipt_status.as_deref(), Some("sending"));
        assert_eq!(message.fields.expect("fields")["_lxmf"]["transient_id"], json!("abcd"));
    }

    #[test]
    fn receipt_and_field_updates_run_on_writer_lane_in_order() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store.insert_message(&outbound_message("msg-1", 1, None)).expect("insert message");

        store.update_receipt_status("msg-1", "sending").expect("update status");
        store
            .update_message_fields("msg-1", Some(&json!({"_lxmf": {"stage": "queued"}})))
            .expect("update fields");

        let message = store.get_message("msg-1").expect("load message").expect("message exists");
        assert_eq!(message.receipt_status.as_deref(), Some("sending"));
        assert_eq!(message.fields.expect("fields")["_lxmf"]["stage"], json!("queued"));
    }

    #[test]
    fn resolve_receipt_status_preserves_terminal_status() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("delivered")))
            .expect("insert delivered message");

        let resolved =
            store.resolve_receipt_status("msg-1", "sent: direct").expect("resolve status");

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

    #[test]
    fn resolve_receipt_status_preserves_sent_over_sending_regression() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-1", 1, Some("sent: propagated resource")))
            .expect("insert sent message");

        let resolved = store
            .resolve_receipt_status("msg-1", "sending: propagated resource")
            .expect("resolve status");

        assert_eq!(resolved.as_deref(), Some("sent: propagated resource"));
        assert_eq!(
            store
                .get_message("msg-1")
                .expect("load message")
                .expect("message exists")
                .receipt_status
                .as_deref(),
            Some("sent: propagated resource")
        );
    }
}
