#[tokio::test]
async fn repeated_path_save_preserves_unchanged_announce_file_and_restores_valid_path() {
    let temp = tempfile::tempdir().expect("storage");
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("repeated-path-save", &identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let learned = learn_cached_path(&transport, iface, "repeated-cache").await;
    let (first_save, concurrent_save) = tokio::join!(
        transport.save_reticulum_path_table(temp.path()),
        transport.save_reticulum_path_table(temp.path()),
    );
    assert_eq!(first_save.expect("initial save"), 1);
    assert_eq!(concurrent_save.expect("concurrent save"), 1);
    let payload = std::fs::read(temp.path().join("destination_table")).expect("path table");
    let entries = PathTable::decode_python_entries(&payload).expect("path rows");
    let path = cached_announce_path(temp.path(), &entries[0].packet_hash);
    let original = std::fs::read(&path).expect("cache bytes");
    let file = std::fs::File::options().write(true).open(&path).expect("cache");
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(946_684_800);
    file.set_times(std::fs::FileTimes::new().set_modified(old)).expect("sentinel timestamp");
    let modified = file.metadata().expect("metadata").modified().expect("modified");
    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("repeat save"), 1);
    assert_eq!(std::fs::read(&path).expect("preserved bytes"), original);
    assert_eq!(std::fs::metadata(&path).expect("metadata").modified().expect("modified"), modified);

    std::fs::remove_file(&path).expect("remove cached announce");
    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("known fast path"), 1);
    assert!(!path.exists(), "unchanged fingerprints skip filesystem reads until verification");

    let mut restart_config = TransportConfig::new("restart-cache", &identity, true);
    restart_config.set_retransmit(true);
    let restarted = Transport::new(restart_config);
    let restarted_iface = *restarted.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restarted_iface, iface);
    handle_announce(
        &learned.packet,
        restarted.get_handler().lock().await,
        restarted_iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert_eq!(restarted.save_reticulum_path_table(temp.path()).await.expect("restart repair"), 1);
    assert_eq!(std::fs::read(&path).expect("repaired missing announce"), original);

    std::fs::write(&path, b"corrupt").expect("corrupt announce cache");
    let mut second_restart_config = TransportConfig::new("second-restart-cache", &identity, true);
    second_restart_config.set_retransmit(true);
    let second_restarted = Transport::new(second_restart_config);
    let second_iface = *second_restarted.iface_manager().lock().await.new_channel(16).address();
    handle_announce(
        &learned.packet,
        second_restarted.get_handler().lock().await,
        second_iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert_eq!(
        second_restarted
            .save_reticulum_path_table(temp.path())
            .await
            .expect("corrupt restart repair"),
        1
    );
    assert_eq!(std::fs::read(&path).expect("repaired corrupt announce"), original);

    let mut config = TransportConfig::new("restored-cache", &identity, true);
    config.set_retransmit(true);
    let restored = Transport::new(config);
    assert_eq!(*restored.iface_manager().lock().await.new_channel(16).address(), iface);
    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 1);
    assert!(restored.has_path(&learned.destination).await);
    assert!(restored.destination_identity(&learned.destination).await.is_some());
}
