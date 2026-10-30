use super::*;

impl MessagesStore {
    /// Release unpinned SQLite pager pages; the bundled shared cache group may
    /// include other connections. This reports calls, never resident bytes.
    pub fn release_unused_sqlite_memory(&self) -> Result<(), std::io::Error> {
        let metrics = &self.write_state.maintenance_metrics;
        metrics.sqlite_cache_release_attempts_total.fetch_add(1, Ordering::Relaxed);
        metrics.sqlite_cache_release.measure(
            || {
                let started = std::time::Instant::now();
                let locked = self.write_state.conn.lock();
                let waited = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                metrics
                    .sqlite_cache_release_lock_wait_ns_total
                    .fetch_add(waited, Ordering::Relaxed);
                metrics.sqlite_cache_release_max_lock_wait_ns.fetch_max(waited, Ordering::Relaxed);
                let conn = locked.map_err(|error| {
                    std::io::Error::other(format!(
                        "SQLite pager-cache release writer mutex poisoned: {error}"
                    ))
                })?;
                self.write_state.write_lock_wait_ns_total.fetch_add(waited, Ordering::Relaxed);
                self.write_state.write_ops_total.fetch_add(1, Ordering::Relaxed);
                if !conn.is_autocommit() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "SQLite pager-cache release requires autocommit after maintenance",
                    ));
                }
                conn.release_memory().map_err(std::io::Error::other)
            },
            |_| 0,
        )
    }
}
