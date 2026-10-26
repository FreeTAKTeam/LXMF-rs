#[test]
fn announce_cache_cleanup_failures_are_visible_and_retryable() {
    use rns_rpc::MetricsProvider;
    let temp = TempDir::new().expect("temp dir");
    let signer = PrivateIdentity::new_from_name("announce-cache-cleanup-retry");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    runtime.block_on(async {
        let transport = Arc::new(Transport::new(TransportConfig::new(
            "announce-cache-cleanup-retry",
            &transport_identity,
            true,
        )));
        let context = crate::announce_persistence::PathTablePersistenceContext::new(
            transport,
            temp.path().to_path_buf(),
        );
        crate::announce_persistence::flush_reticulum_path_table(&context)
            .await
            .expect("initial persisted snapshot");

        let announce_dir = temp.path().join("cache").join("announces");
        fs::remove_dir_all(&announce_dir).expect("remove announce directory");
        fs::write(&announce_dir, b"not a directory").expect("replace directory with file");
        crate::announce_persistence::maintain_announce_cache(&context).await;
        let failed = context.metrics().snapshot();
        assert_eq!(failed["cleanup_errors_total"], 1);
        assert_eq!(failed["in_flight"], false);

        fs::remove_file(&announce_dir).expect("remove invalid directory fixture");
        fs::create_dir(&announce_dir).expect("restore announce directory");
        crate::announce_persistence::maintain_announce_cache(&context).await;
        let recovered = context.metrics().snapshot();
        assert_eq!(recovered["cleanup_errors_total"], 1, "the earlier failure remains counted");
        assert!(recovered["cleanup_last_success_unix_secs"].as_u64().unwrap_or(0) > 0);
        assert_eq!(recovered["in_flight"], false);
    });
}

#[tokio::test(start_paused = true)]
async fn announce_cache_ticks_never_flush_and_dirty_triggers_are_rate_limited() {
    use rns_rpc::MetricsProvider;
    let temp = TempDir::new().expect("temp dir");
    let signer = PrivateIdentity::new_from_name("announce-save-timer");
    let identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new("timer", &identity, true)));
    let context = crate::announce_persistence::PathTablePersistenceContext::new(
        transport, temp.path().to_path_buf(),
    );
    crate::announce_persistence::maintain_announce_cache(&context).await;
    assert_eq!(context.metrics().snapshot()["runs_total"], 0);
    let tx = crate::announce_persistence::spawn_path_table_persistence_worker_with_interval(
        context.clone(), std::time::Duration::from_secs(300),
    );
    tx.send(()).await.expect("trigger");
    tokio::task::yield_now().await;
    tx.send(()).await.expect("burst trigger");
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(30)).await;
    tokio::task::yield_now().await;
    assert_eq!(context.metrics().snapshot()["runs_total"], 0, "maintenance never saves");
    tokio::time::advance(std::time::Duration::from_secs(269)).await;
    tokio::task::yield_now().await;
    assert_eq!(context.metrics().snapshot()["runs_total"], 0);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    // Filesystem operations run on the blocking pool, independent of paused time.
    for _ in 0..100_000 {
        if context.metrics().snapshot()["runs_total"] == 1 { break; }
        tokio::task::yield_now().await;
    }
    assert_eq!(context.metrics().snapshot()["runs_total"], 1);
    tx.send(()).await.expect("next trigger");
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(299)).await;
    tokio::task::yield_now().await;
    assert_eq!(context.metrics().snapshot()["runs_total"], 1);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    for _ in 0..100_000 {
        if context.metrics().snapshot()["runs_total"] == 2 { break; }
        tokio::task::yield_now().await;
    }
    assert_eq!(context.metrics().snapshot()["runs_total"], 2);
    drop(tx);
}
