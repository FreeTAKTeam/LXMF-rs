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
            match tokio::task::spawn_blocking(move || maintenance_daemon.maintain_storage_wal())
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => log::warn!("[daemon] storage WAL maintenance failed: {error}"),
                Err(error) => log::error!("[daemon] storage WAL maintenance task failed: {error}"),
            }
        }
    });
}

fn spawn_propagation_storage_maintenance(daemon: Arc<RpcDaemon>) {
    tokio::spawn(async move {
        let mut last_enabled = false;
        let mut interval_secs = 0;
        let mut next_maintenance = tokio::time::Instant::now();
        loop {
            let state = daemon.current_propagation_state();
            let configured_interval_secs = state.storage_maintenance_interval_secs.max(1);
            if state.enabled != last_enabled || configured_interval_secs != interval_secs {
                last_enabled = state.enabled;
                interval_secs = configured_interval_secs;
                next_maintenance =
                    tokio::time::Instant::now() + Duration::from_secs(configured_interval_secs);
            }

            if !state.enabled {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }

            let now = tokio::time::Instant::now();
            if now >= next_maintenance {
                let maintenance_daemon = Arc::clone(&daemon);
                // Join each storage operation before scheduling another one;
                // database contention must not block the network reactor.
                match tokio::task::spawn_blocking(move || -> Result<usize, std::io::Error> {
                    let pruned = maintenance_daemon.maintain_propagation_storage()?;
                    maintenance_daemon.release_unused_storage_memory()?;
                    Ok(pruned)
                })
                .await
                {
                    Ok(Ok(pruned)) if pruned > 0 => {
                        log::info!(
                            "[daemon] propagation storage maintenance pruned peer entries={pruned}"
                        );
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        log::error!("[daemon] propagation storage maintenance failed: {error}");
                    }
                    Err(error) => {
                        log::error!(
                            "[daemon] propagation storage maintenance worker failed: {error}"
                        );
                    }
                }
                next_maintenance = tokio::time::Instant::now() + Duration::from_secs(interval_secs);
                continue;
            }

            tokio::time::sleep(next_maintenance.duration_since(now).min(Duration::from_secs(1)))
                .await;
        }
    });
}
