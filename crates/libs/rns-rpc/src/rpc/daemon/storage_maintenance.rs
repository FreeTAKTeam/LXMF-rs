use super::*;

impl RpcDaemon {
    /// Periodic WAL maintenance entry point for the daemon binary's timer. Logs a
    /// truncation at info and a reader-pinned WAL at debug; sizes only, no payloads.
    pub fn maintain_storage_wal(
        &self,
    ) -> Result<crate::storage::broker::WalMaintenanceOutcome, std::io::Error> {
        use crate::storage::operation_metrics::WalCheckpointOutcome;
        let outcome = self.store.maintain_wal().map_err(std::io::Error::other)?;
        match outcome.outcome {
            WalCheckpointOutcome::Truncated => log::info!(
                "[daemon] storage WAL truncated bytes_before={} bytes_after={}",
                outcome.size_before,
                outcome.size_after
            ),
            WalCheckpointOutcome::Busy | WalCheckpointOutcome::PassiveIncomplete => log::debug!(
                "[daemon] storage WAL checkpoint deferred outcome={:?} bytes={}",
                outcome.outcome,
                outcome.size_before
            ),
            WalCheckpointOutcome::BelowThreshold => {}
        }
        Ok(outcome)
    }
}
