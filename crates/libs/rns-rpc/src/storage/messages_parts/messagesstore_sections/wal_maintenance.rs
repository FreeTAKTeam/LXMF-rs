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
        Ok(())
    }

    /// Routine WAL maintenance for the periodic daemon timer: checkpoint and
    /// truncate once the file exceeds `max(64 MiB, budget/64)`, reporting busy
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
        let result = self.with_control_conn(|conn| super::broker::wal_maintenance_run(conn, threshold));
        match &result {
            Ok(outcome) => {
                self.write_state.wal_metrics.record_outcome(outcome.outcome, outcome.size_after);
            }
            Err(_) => self.write_state.wal_metrics.record_error(),
        }
        result
    }
}
