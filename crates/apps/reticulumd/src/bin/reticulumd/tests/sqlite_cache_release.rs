#[tokio::test(start_paused = true)]
async fn sqlite_cache_release_timer_preserves_manual_and_disabled_paths() {
    let daemon = std::sync::Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.configure_propagation_node(
        true,
        false,
        None,
        false,
        None,
        500,
        500,
        16,
        3,
        0,
        Vec::new(),
        Some(64),
        100,
        100,
        3600,
        3600,
        10,
        300,
    );
    daemon.set_propagation_state(false, None, 16);
    let releases =
        || daemon.metrics_snapshot()["storage"]["maintenance"]["sqlite_cache_release"].clone();
    bootstrap::spawn_storage_maintenance(std::sync::Arc::clone(&daemon));
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(600)).await;
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    assert_eq!(releases()["attempts_total"], 0, "disabled propagation must not release");

    daemon.maintain_propagation_storage().expect("manual maintenance");
    assert_eq!(releases()["attempts_total"], 0, "manual behavior is unchanged");
    daemon.set_propagation_state(true, None, 16);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(300)).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if releases()["runs_total"] == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "periodic cache release did not finish");
        tokio::task::yield_now().await;
    }
    let counters = releases();
    assert_eq!(counters["attempts_total"], 1);
    assert_eq!(counters["runs_total"], 1);
    assert_eq!(counters["errors_total"], 0);
    let phases = daemon.metrics_snapshot()["storage"]["maintenance"].clone();
    for phase in ["expiry", "quota", "payload"] {
        assert_eq!(phases[phase]["runs_total"], 2, "release follows every original phase");
    }
    daemon.set_propagation_state(false, None, 16);
    tokio::time::advance(std::time::Duration::from_secs(600)).await;
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    assert_eq!(releases()["attempts_total"], 1, "disabled timer must stop releasing");
}
