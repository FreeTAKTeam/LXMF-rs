//! Fixed statement families; no SQL parameters, peer IDs, or payloads are retained.
use rusqlite::{Statement, StatementStatus};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

#[derive(Default)]
pub(super) struct QueryMetrics {
    calls: AtomicU64,
    errors: AtomicU64,
    wall_ns: AtomicU64,
    max_wall_ns: AtomicU64,
    vm_steps: AtomicU64,
    fullscan_steps: AtomicU64,
    sorts: AtomicU64,
    autoindex_rows: AtomicU64,
}

impl QueryMetrics {
    pub fn measure<T>(
        &self,
        statement: &mut Statement<'_>,
        run: impl FnOnce(&mut Statement<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let started = Instant::now();
        let result = run(statement);
        let elapsed = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.calls.fetch_add(1, Relaxed);
        self.errors.fetch_add(u64::from(result.is_err()), Relaxed);
        self.wall_ns.fetch_add(elapsed, Relaxed);
        self.max_wall_ns.fetch_max(elapsed, Relaxed);
        for (counter, status) in [
            (&self.vm_steps, StatementStatus::VmStep),
            (&self.fullscan_steps, StatementStatus::FullscanStep),
            (&self.sorts, StatementStatus::Sort),
            (&self.autoindex_rows, StatementStatus::AutoIndex),
        ] {
            counter.fetch_add(statement.reset_status(status).max(0) as u64, Relaxed);
        }
        result
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "calls_total": self.calls.load(Relaxed),
            "errors_total": self.errors.load(Relaxed),
            "wall_ns_total": self.wall_ns.load(Relaxed),
            "max_wall_ns": self.max_wall_ns.load(Relaxed),
            "vm_steps_total": self.vm_steps.load(Relaxed),
            "fullscan_steps_total": self.fullscan_steps.load(Relaxed),
            "sorts_total": self.sorts.load(Relaxed),
            "autoindex_rows_total": self.autoindex_rows.load(Relaxed),
        })
    }
}

#[derive(Default)]
pub(super) struct AnnounceMetrics {
    pub history: QueryMetrics,
    pub stamp_cost: QueryMetrics,
    pub candidates: QueryMetrics,
    pub window: QueryMetrics,
    pub count: QueryMetrics,
    pub prune: QueryMetrics,
    pub maintenance: super::operation_metrics::MaintenancePhaseMetrics,
    pub retained_at_last_sweep: AtomicU64,
    pub examined_total: AtomicU64,
}

impl AnnounceMetrics {
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "history": self.history.snapshot(),
            "stamp_cost": self.stamp_cost.snapshot(),
            "retention_candidates": self.candidates.snapshot(),
            "retention_window": self.window.snapshot(),
            "retention_count": self.count.snapshot(),
            "retention_delete": self.prune.snapshot(),
            "maintenance": self.maintenance.snapshot(),
            "retained_at_last_sweep": self.retained_at_last_sweep.load(Relaxed),
            "examined_total": self.examined_total.load(Relaxed),
        })
    }
}
