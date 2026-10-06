#[tokio::test]
async fn repeated_path_save_preserves_unchanged_announce_file_and_restores_valid_path() {
    let temp = tempfile::tempdir().expect("storage");
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("repeated-path-save", &identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let learned = learn_cached_path(&transport, iface, "repeated-cache").await;
    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("initial save"), 1);
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
    let mut config = TransportConfig::new("restored-cache", &identity, true);
    config.set_retransmit(true);
    let restored = Transport::new(config);
    assert_eq!(*restored.iface_manager().lock().await.new_channel(16).address(), iface);
    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 1);
    assert!(restored.has_path(&learned.destination).await);
    assert!(restored.destination_identity(&learned.destination).await.is_some());
}
