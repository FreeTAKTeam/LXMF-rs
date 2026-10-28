// Operation-level storage counters for issue 663: propagation refill work and
// WAL maintenance. Monotonic totals plus last-run gauges; no payloads, peers
// or SQL values. Read live by the daemon metrics snapshot.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct RefillMetrics {
    pub in_flight: AtomicU64,
    pub invocations_total: AtomicU64,
    pub skipped_complete_total: AtomicU64,
    pub skipped_capacity_total: AtomicU64,
    pub pages_total: AtomicU64,
    pub candidates_examined_total: AtomicU64,
    pub rows_inserted_total: AtomicU64,
    pub capacity_reached_total: AtomicU64,
    pub wall_ns_total: AtomicU64,
    pub write_lock_wait_ns_total: AtomicU64,
    pub max_page_ns: AtomicU64,
    pub priority_yields_total: AtomicU64,
}

impl RefillMetrics {
    pub fn track_in_flight(&self) -> InFlightGuard<'_> {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        InFlightGuard(&self.in_flight)
    }

    pub fn record_page(&self, candidates: usize, inserted: usize, page_ns: u64) {
        self.pages_total.fetch_add(1, Ordering::Relaxed);
        self.candidates_examined_total.fetch_add(candidates as u64, Ordering::Relaxed);
        self.rows_inserted_total.fetch_add(inserted as u64, Ordering::Relaxed);
        self.max_page_ns.fetch_max(page_ns, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "in_flight": self.in_flight.load(Ordering::Relaxed),
            "invocations_total": self.invocations_total.load(Ordering::Relaxed),
            "skipped_complete_total": self.skipped_complete_total.load(Ordering::Relaxed),
            "skipped_capacity_total": self.skipped_capacity_total.load(Ordering::Relaxed),
            "pages_total": self.pages_total.load(Ordering::Relaxed),
            "candidates_examined_total": self.candidates_examined_total.load(Ordering::Relaxed),
            "rows_inserted_total": self.rows_inserted_total.load(Ordering::Relaxed),
            "capacity_reached_total": self.capacity_reached_total.load(Ordering::Relaxed),
            "wall_ns_total": self.wall_ns_total.load(Ordering::Relaxed),
            "write_lock_wait_ns_total": self.write_lock_wait_ns_total.load(Ordering::Relaxed),
            "max_page_ns": self.max_page_ns.load(Ordering::Relaxed),
            "priority_yields_total": self.priority_yields_total.load(Ordering::Relaxed),
        })
    }
}

pub struct InFlightGuard<'a>(&'a AtomicU64);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Outcome of one WAL checkpoint attempt, as reported by SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalCheckpointOutcome {
    /// WAL was below the threshold; nothing attempted.
    BelowThreshold,
    /// `TRUNCATE` completed and the WAL file was reset.
    Truncated,
    /// A reader or writer held the WAL; SQLite reported busy and nothing changed.
    Busy,
    /// `PASSIVE` ran but some frames remain uncheckpointed.
    PassiveIncomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct WalCheckpointFrames {
    pub busy: i64,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
}

#[derive(Debug, Default)]
pub struct WalMetrics {
    /// Every maintenance pass, including below-threshold ones, so a live timer is
    /// distinguishable from a stalled one when no checkpoint is due.
    pub runs_total: AtomicU64,
    pub checkpoint_attempts_total: AtomicU64,
    pub truncated_total: AtomicU64,
    pub busy_total: AtomicU64,
    pub passive_incomplete_total: AtomicU64,
    pub errors_total: AtomicU64,
    pub emergency_reclaim_total: AtomicU64,
    pub last_wal_bytes: AtomicU64,
    pub last_checkpoint_unix_secs: AtomicU64,
    last_checkpoint: std::sync::Mutex<Option<WalCheckpointFrames>>,
}

impl WalMetrics {
    pub fn record_checkpoint(&self, frames: WalCheckpointFrames) {
        *self.last_checkpoint.lock().expect("WAL checkpoint metrics poisoned") = Some(frames);
    }

    pub fn record_outcome(&self, outcome: WalCheckpointOutcome, wal_bytes_after: u64) {
        self.runs_total.fetch_add(1, Ordering::Relaxed);
        self.last_wal_bytes.store(wal_bytes_after, Ordering::Relaxed);
        match outcome {
            WalCheckpointOutcome::BelowThreshold => return,
            WalCheckpointOutcome::Truncated => {
                self.truncated_total.fetch_add(1, Ordering::Relaxed);
                self.last_checkpoint_unix_secs.store(unix_now_secs(), Ordering::Relaxed);
            }
            WalCheckpointOutcome::Busy => {
                self.busy_total.fetch_add(1, Ordering::Relaxed);
            }
            WalCheckpointOutcome::PassiveIncomplete => {
                self.passive_incomplete_total.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.checkpoint_attempts_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.checkpoint_attempts_total.fetch_add(1, Ordering::Relaxed);
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "runs_total": self.runs_total.load(Ordering::Relaxed),
            "checkpoint_attempts_total": self.checkpoint_attempts_total.load(Ordering::Relaxed),
            "truncated_total": self.truncated_total.load(Ordering::Relaxed),
            "busy_total": self.busy_total.load(Ordering::Relaxed),
            "passive_incomplete_total": self.passive_incomplete_total.load(Ordering::Relaxed),
            "errors_total": self.errors_total.load(Ordering::Relaxed),
            "emergency_reclaim_total": self.emergency_reclaim_total.load(Ordering::Relaxed),
            "last_wal_bytes": self.last_wal_bytes.load(Ordering::Relaxed),
            "last_checkpoint": *self.last_checkpoint.lock().expect("WAL checkpoint metrics poisoned"),
            "last_checkpoint_unix_secs": self.last_checkpoint_unix_secs.load(Ordering::Relaxed),
        })
    }
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refill_page_records_counts_and_max() {
        let metrics = RefillMetrics::default();
        metrics.record_page(128, 3, 500);
        metrics.record_page(4, 0, 900);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot["pages_total"], 2);
        assert_eq!(snapshot["candidates_examined_total"], 132);
        assert_eq!(snapshot["rows_inserted_total"], 3);
        assert_eq!(snapshot["max_page_ns"], 900);
    }

    #[test]
    fn wal_outcomes_are_counted_separately() {
        let metrics = WalMetrics::default();
        metrics.record_outcome(WalCheckpointOutcome::BelowThreshold, 10);
        metrics.record_outcome(WalCheckpointOutcome::Busy, 20);
        metrics.record_outcome(WalCheckpointOutcome::Truncated, 0);
        metrics.record_error();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot["checkpoint_attempts_total"], 3);
        assert_eq!(snapshot["busy_total"], 1);
        assert_eq!(snapshot["truncated_total"], 1);
        assert_eq!(snapshot["errors_total"], 1);
        assert_eq!(snapshot["last_wal_bytes"], 0);
        assert!(snapshot["last_checkpoint_unix_secs"].as_u64().unwrap_or(0) > 0);
    }
}
