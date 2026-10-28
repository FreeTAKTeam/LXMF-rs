#[tokio::test]
async fn path_save_report_splits_written_skipped_and_verified_records() {
    let temp = tempfile::tempdir().expect("storage");
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("path-save-report", &identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let _learned = learn_cached_path(&transport, iface, "save-report").await;

    let first = transport.save_reticulum_path_table_report(temp.path()).await.expect("first save");
    assert_eq!(first.kept_entries, 1);
    assert_eq!(first.records_examined, 1);
    assert!(first.full_verification, "first save verifies against disk");
    assert_eq!(first.written, 1);
    assert_eq!(first.skipped_known, 0);
    assert_eq!(first.fingerprint_map_len, 1);
    assert!(first.duration_ns > 0);

    let second = transport.save_reticulum_path_table_report(temp.path()).await.expect("repeat");
    assert_eq!(second.records_examined, 1);
    assert_eq!(second.skipped_known, 1, "known fingerprint skips serialize/open/read/write");
    assert_eq!(second.written, 0);
    assert!(!second.full_verification);

    let tables = transport.table_occupancy().await;
    assert_eq!(tables.path_entries, 1);
    assert_eq!(tables.single_out_destinations, 1);
    assert_eq!(tables.random_blob_count, 1);
    assert_eq!(tables.fingerprint_map_len, Some(1));
    assert!(tables.announce_retransmit_entries + tables.announce_cache_entries >= 1);
    assert_eq!(tables.announce_cache_capacity, 100_000);
    assert_eq!(transport.table_occupancy_now().expect("tables idle"), tables);
}

#[tokio::test]
async fn restore_seeds_canonical_cache_fingerprints() {
    let temp = tempfile::tempdir().expect("storage");
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("restore-seed", &identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let _learned = learn_cached_path(&transport, iface, "seed").await;
    transport.save_reticulum_path_table(temp.path()).await.expect("save");
    let mut config = TransportConfig::new("restore-seed", &identity, true);
    config.set_retransmit(true);
    let restored = Transport::new(config);
    assert_eq!(*restored.iface_manager().lock().await.new_channel(16).address(), iface);
    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 1);
    assert_eq!(restored.table_occupancy_now().expect("restored tables").fingerprint_map_len, Some(1), "restored fingerprints are visible before the first save");
    let report = restored.save_reticulum_path_table_report(temp.path()).await.expect("save restored");
    assert_eq!(report.skipped_known, 1, "validated restore needs no file open on save: {report:?}");
    assert_eq!(report.written, 0);
    assert_eq!(report.verified_unchanged, 0);
    assert!(!report.full_verification);
}
