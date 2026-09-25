#[tokio::test]
async fn reticulum_path_table_restore_skips_blackholed_cached_announce_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let learned = learn_cached_path(&transport, iface, "blackholed-restore").await;
    let identity = transport
        .destination_identity(&learned.destination)
        .await
        .expect("learned route identity");
    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic interface hashes");
    assert_eq!(restored.set_identity_blackholed(identity.address_hash, true).await, 0);

    let report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(report.restored_active_paths, 0, "blackholed route must not be restored");
    assert_eq!(report.skipped.active_blackholed_identity, 1);
    assert!(!restored.has_path(&learned.destination).await);
    assert!(restored.destination_identity(&learned.destination).await.is_none());
}
