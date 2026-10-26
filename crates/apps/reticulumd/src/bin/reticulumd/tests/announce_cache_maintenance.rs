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
