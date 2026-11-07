pub use r3akt_event_broker::storage::wal_maintenance::{
    checkpoint_truncate_nonblocking, maintain, truncate_limit_bytes, wal_file_size,
    WalMaintenanceOutcome,
};
/// Shared cadence for the daemon timer and emergency write-path checkpoints.
pub fn wal_maintenance_interval() -> std::time::Duration {
    let seconds = match std::env::var("LXMD_STORAGE_WAL_MAINTENANCE_INTERVAL_SECS") {
        Ok(value) => match value.parse::<u64>() {
            Ok(seconds) => seconds.max(1),
            Err(error) => {
                log::warn!("invalid LXMD_STORAGE_WAL_MAINTENANCE_INTERVAL_SECS: {error}");
                60
            }
        },
        Err(_) => 60,
    };
    std::time::Duration::from_secs(seconds)
}
