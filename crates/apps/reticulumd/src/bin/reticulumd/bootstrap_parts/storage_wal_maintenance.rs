/// Shared with write-path checkpoint throttling in the storage owner.
pub(super) fn storage_wal_maintenance_interval() -> Duration {
    rns_rpc::wal_maintenance_interval()
}

/// Spawn both storage timers: propagation storage maintenance (existing) and the
/// WAL checkpoint/truncate pass that keeps the broker database's WAL bounded.
pub(super) fn spawn_storage_maintenance(daemon: Arc<RpcDaemon>) {
    spawn_storage_wal_maintenance(daemon.clone(), storage_wal_maintenance_interval());
    spawn_propagation_storage_maintenance(daemon);
}

fn spawn_storage_wal_maintenance(daemon: Arc<RpcDaemon>, interval: Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let maintenance_daemon = Arc::clone(&daemon);
            // Join the blocking pass before scheduling another; database work
            // must not run on the network reactor.
            match tokio::task::spawn_blocking(move || maintenance_daemon.maintain_storage_wal()).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => log::warn!("[daemon] storage WAL maintenance failed: {error}"),
                Err(error) => log::error!("[daemon] storage WAL maintenance task failed: {error}"),
            }
        }
    });
}
