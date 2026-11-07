impl MessagesStore {
    /// Re-read `broker_meta.budget` into the lock-free cache used by the per-write
    /// WAL emergency check. Zero means the durable broker is disabled.
    pub(super) fn refresh_broker_budget_cache(&self) -> rusqlite::Result<()> {
        let budget = self.with_control_conn(|conn| {
            if super::broker::enabled(conn)? {
                conn.query_row("SELECT budget FROM broker_meta", [], |row| row.get::<_, i64>(0))
            } else {
                Ok(0)
            }
        })?;
        self.write_state.broker_budget_bytes.store(budget.max(0) as u64, Ordering::Relaxed);
        self.durable_broker_enabled.store(budget > 0, Ordering::Release);
        Ok(())
    }

    /// Routine WAL maintenance for the periodic daemon timer: checkpoint and
    /// truncate once the file reaches `max(64 MiB, budget/64)`, reporting busy
    /// rather than waiting on readers. Outcomes feed `storage.wal` metrics.
    pub fn maintain_wal(&self) -> rusqlite::Result<super::broker::WalMaintenanceOutcome> {
        let budget = self.write_state.broker_budget_bytes.load(Ordering::Relaxed);
        let threshold =
            super::broker::truncate_limit_bytes((budget > 0).then_some(budget));
        self.maintain_wal_with_threshold(threshold)
    }

    pub fn maintain_wal_with_threshold(
        &self,
        threshold: u64,
    ) -> rusqlite::Result<super::broker::WalMaintenanceOutcome> {
        let result = if let Some(maintenance) = &self.maintenance_conn {
            let conn = maintenance.lock().expect("messages sqlite maintenance mutex poisoned");
            super::broker::wal_maintenance_run(&conn, threshold)
        } else {
            // In-memory stores have no WAL or separate database connection.
            self.with_control_conn(|conn| super::broker::wal_maintenance_run(conn, threshold))
        };
        match &result {
            Ok(outcome) => {
                self.write_state.wal_metrics.record_outcome(outcome.outcome, outcome.size_after);
                if let Some(frames) = outcome.checkpoint {
                    self.write_state.wal_metrics.record_checkpoint(frames);
                }
            }
            Err(_) => self.write_state.wal_metrics.record_error(),
        }
        result
    }
}

impl MessagesStore {
    /// Rate-limited emergency WAL guard without SQL: the cached budget decides whether the
    /// file is even checked, and only a WAL past budget/8 triggers a non-blocking
    /// TRUNCATE. Routine shrinking belongs to `maintain_wal`; this path exists so
    /// broker-full recovery after a pinned reader retries at the maintenance interval.
    fn reclaim_wal_recorded(write_state: &WriteState, conn: &Connection) -> rusqlite::Result<()> {
        let budget = write_state.broker_budget_bytes.load(Ordering::Relaxed);
        if budget == 0 || !conn.is_autocommit() {
            return Ok(());
        }
        let size = super::broker::wal_file_size(conn)?;
        if size <= budget / 8 {
            if size > 0 {
                write_state.wal_metrics.last_wal_bytes.store(size, Ordering::Relaxed);
            }
            return Ok(());
        }
        let mut last = write_state.last_wal_reclaim.lock().expect("WAL reclaim clock poisoned");
        if last.is_some_and(|last| last.elapsed() < write_state.wal_reclaim_interval) {
            return Ok(());
        }
        *last = Some(std::time::Instant::now());
        drop(last);
        write_state.wal_metrics.emergency_reclaim_total.fetch_add(1, Ordering::Relaxed);
        match super::broker::checkpoint_truncate_nonblocking(conn) {
            Ok((outcome, frames)) => {
                write_state.wal_metrics.record_checkpoint(frames);
                let after = super::broker::wal_file_size(conn)?;
                write_state.wal_metrics.record_outcome(outcome, after);
                Ok(())
            }
            Err(error) => {
                write_state.wal_metrics.record_error();
                Err(error)
            }
        }
    }

}
