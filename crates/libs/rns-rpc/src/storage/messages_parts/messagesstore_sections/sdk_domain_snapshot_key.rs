impl MessagesStore {

    const SDK_DOMAIN_SNAPSHOT_KEY: &'static str = "sdk_domains.v1";

    pub(super) fn is_terminal_receipt_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("failed")
            || matches!(normalized.as_str(), "cancelled" | "delivered" | "expired" | "rejected")
    }

    pub(super) fn should_preserve_receipt_status(existing_status: &str, candidate_status: &str) -> bool {
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
            refill_metrics: Default::default(),
            wal_metrics: Default::default(),
            maintenance_metrics: Default::default(),
            announce_metrics: Default::default(),
            priority_waiters: AtomicUsize::new(0),
            priority_gate: (Mutex::new(()), Condvar::new()),
            broker_budget_bytes: AtomicU64::new(0),
            last_wal_reclaim: Mutex::new(None),
            wal_reclaim_interval: super::broker::wal_maintenance_interval(),
        });
        let (outbound_write_tx, outbound_write_rx) = WriteSender::channel();
        let store = Self {
            durable_broker_enabled: AtomicBool::new(false),
            write_state: write_state.clone(),
            outbound_write_tx,
            writer_thread: Some(Self::spawn_outbound_write_worker(write_state, outbound_write_rx)),
            read_conn: None,
            maintenance_conn: None,
            read_lock_wait_ns_total: AtomicU64::new(0),
            read_ops_total: AtomicU64::new(0),
        };
        store.configure_connection()?;
        store.with_control_conn(super::broker::restore_profile)?;
        store.refresh_broker_budget_cache()?;
        store.init_schema()?;
        store.refresh_message_count_cache()?;
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
            refill_metrics: Default::default(),
            wal_metrics: Default::default(),
            maintenance_metrics: Default::default(),
            announce_metrics: Default::default(),
            priority_waiters: AtomicUsize::new(0),
            priority_gate: (Mutex::new(()), Condvar::new()),
            broker_budget_bytes: AtomicU64::new(0),
            last_wal_reclaim: Mutex::new(None),
            wal_reclaim_interval: super::broker::wal_maintenance_interval(),
        });
        let (outbound_write_tx, outbound_write_rx) = WriteSender::channel();
        let store = Self {
            durable_broker_enabled: AtomicBool::new(false),
            write_state: write_state.clone(),
            outbound_write_tx,
            writer_thread: Some(Self::spawn_outbound_write_worker(write_state, outbound_write_rx)),
            read_conn: Some(Mutex::new(read_conn)),
            maintenance_conn: Some(Mutex::new(Connection::open_with_flags(
                path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
            )?)),
            read_lock_wait_ns_total: AtomicU64::new(0),
            read_ops_total: AtomicU64::new(0),
        };
        store.configure_connection()?;
        store.with_control_conn(super::broker::restore_profile)?;
        store.refresh_broker_budget_cache()?;
        store.init_schema()?;
        store.refresh_message_count_cache()?;
        Ok(store)
    }

    fn refresh_message_count_cache(&self) -> rusqlite::Result<()> {
        let count: i64 = self.with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        })?;
        self.write_state.message_count_cache.store(count.max(0) as u64, Ordering::Relaxed);
        Ok(())
    }

    fn with_write_conn<T>(&self,f:impl FnOnce(&Connection)->rusqlite::Result<T>)->rusqlite::Result<T> {
        self.with_control_conn(|conn| {super::broker::mutation_admission(conn,true)?; f(conn)})
    }
    fn with_control_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let started = std::time::Instant::now();
        let conn = self.write_state.conn.lock().expect("messages sqlite write mutex poisoned");
        let waited_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.write_state.write_lock_wait_ns_total.fetch_add(waited_ns, Ordering::Relaxed);
        self.write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
        Self::reclaim_wal_recorded(&self.write_state, &conn)?;
        f(&conn).map_err(super::broker::map_commit_error)
    }

    /// Counts a broker/control writer while it waits for the connection so refill
    /// pages can yield instead of racing it for the unfair mutex.
    fn write_lock_priority<'a>(
        write_state: &'a WriteState,
    ) -> std::sync::MutexGuard<'a, Connection> {
        write_state.priority_waiters.fetch_add(1, Ordering::AcqRel);
        let guard = write_state.conn.lock().expect("messages sqlite write mutex poisoned");
        write_state.priority_waiters.fetch_sub(1, Ordering::AcqRel);
        // Wake any refill parked in `yield_to_priority_writers`; the gate mutex is
        // taken so the notification cannot slip between its check and its wait.
        let _gate = write_state.priority_gate.0.lock().expect("priority gate mutex poisoned");
        write_state.priority_gate.1.notify_all();
        guard
    }

    /// Called between refill pages: while a priority writer waits, park on the
    /// gate condvar (a bare yield lets this thread win the futex straight back)
    /// until that writer holds the connection, in 5 ms slices for at most ~50 ms.
    /// Returns how many waits were taken.
    pub(super) fn yield_to_priority_writers(&self) -> u32 {
        let steps = self.wait_for_priority_writers();
        if steps > 0 {
            self.write_state
                .refill_metrics
                .priority_yields_total
                .fetch_add(1, Ordering::Relaxed);
        }
        steps
    }

    /// Let a priority writer acquire the shared connection without attributing
    /// maintenance waits to the propagation-refill metrics.
    pub(super) fn wait_for_priority_writers(&self) -> u32 {
        let (gate, signal) = &self.write_state.priority_gate;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
        let mut steps = 0u32;
        loop {
            let guard = gate.lock().expect("priority gate mutex poisoned");
            if self.write_state.priority_waiters.load(Ordering::Acquire) == 0
                || std::time::Instant::now() >= deadline
            {
                break;
            }
            let (_guard, _) = signal
                .wait_timeout(guard, std::time::Duration::from_millis(5))
                .expect("priority gate mutex poisoned");
            steps += 1;
        }
        steps
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
            self.with_control_conn(f)
        }
    }

    /// Refill and WAL operation counters for the daemon metrics snapshot.
    pub fn operation_metrics_snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "refill": self.write_state.refill_metrics.snapshot(),
            "wal": self.write_state.wal_metrics.snapshot(),
            "maintenance": self.write_state.maintenance_metrics.snapshot(),
            "announces": self.write_state.announce_metrics.snapshot(),
        })
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

    fn write_lock_and_run<T>(write_state:&WriteState,f:impl FnOnce(&Connection)->rusqlite::Result<T>)->rusqlite::Result<T> {
        Self::write_lock_and_run_base(write_state,|conn|{super::broker::mutation_admission(conn,true)?; f(conn)})
    }
    fn write_lock_and_run_control<T>(write_state:&WriteState,f:impl FnOnce(&Connection)->rusqlite::Result<T>)->rusqlite::Result<T> {
        Self::write_lock_and_run_base(write_state,|conn|super::broker::single_transaction(conn,f))
    }
    fn write_lock_and_run_base<T>(
        write_state: &WriteState,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let started = std::time::Instant::now();
        let conn = Self::write_lock_priority(write_state);
        let waited_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        write_state.write_lock_wait_ns_total.fetch_add(waited_ns, Ordering::Relaxed);
        write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
        Self::reclaim_wal_recorded(write_state, &conn)?;
        f(&conn).map_err(super::broker::map_commit_error)
    }

    fn insert_message_direct(
        write_state: &WriteState,
        record: &MessageRecord,
    ) -> rusqlite::Result<()> {
        let fields_json = record.fields.as_ref().map(serialize_json_for_sql).transpose()?;
        Self::write_lock_and_run_control(write_state, |conn| {
            if super::broker::enabled(conn)? {
                let tx = conn.unchecked_transaction()?;
                let inserted = super::broker::insert_atomic(&tx, record, None)?;
                tx.commit()?;
                if inserted { write_state.message_count_cache.fetch_add(1, Ordering::Relaxed); }
                return Ok(());
            }
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
        Self::write_lock_and_run_control(write_state, |conn| {
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
            if super::broker::enabled(conn)? {
                let tx = conn.unchecked_transaction()?;
                super::broker::status_atomic(&tx, message_id, candidate_status)?;
                tx.commit()?;
            } else {
                conn.execute("UPDATE messages SET receipt_status = ?1 WHERE id = ?2", params![candidate_status,message_id])?;
            }
            Ok(Some(candidate_status.to_string()))
        })
    }

    fn update_receipt_status_direct(
        write_state: &WriteState,
        message_id: &str,
        status: &str,
    ) -> rusqlite::Result<()> {
        Self::write_lock_and_run_control(write_state,|conn| {
            if super::broker::enabled(conn)? {
                let tx=conn.unchecked_transaction()?;
                super::broker::status_atomic(&tx,message_id,status)?;
                tx.commit()?;
            } else { conn.execute("UPDATE messages SET receipt_status=?1 WHERE id=?2",params![status,message_id])?; }
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
            lxmf_propagation::identities::upsert(conn, peer, public_key_hex, verifying_key_hex, updated_at)
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
        let reservation=self.outbound_write_tx.reserve(message_retained_bytes(record)+512).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send_reserved(OutboundWriteCommand::InsertMessage { record: record.clone(), reply: reply_tx },reservation)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }
}
