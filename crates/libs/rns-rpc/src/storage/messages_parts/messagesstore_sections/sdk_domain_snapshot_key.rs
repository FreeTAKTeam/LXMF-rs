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
        let fields_json = record.fields.as_ref().map(serialize_json_for_sql).transpose()?;
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
        let capabilities_json = serialize_json_for_sql(&record.capabilities)?;
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

    fn upsert_announce_identity_direct(
        write_state: &WriteState,
        peer: &str,
        public_key_hex: &str,
        verifying_key_hex: &str,
        updated_at: i64,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run(write_state, |conn| {
            conn.execute(
                "INSERT INTO announce_identities
                    (peer, public_key_hex, verifying_key_hex, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(peer) DO UPDATE SET
                    public_key_hex = excluded.public_key_hex,
                    verifying_key_hex = excluded.verifying_key_hex,
                    updated_at = excluded.updated_at",
                params![peer, public_key_hex, verifying_key_hex, updated_at],
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
}
