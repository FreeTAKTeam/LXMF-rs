#[tokio::test]
async fn expired_persisted_route_is_not_reused_before_fresh_announce_recovery() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut initial_config = TransportConfig::new("route-restart", &local_identity, true);
    initial_config.set_retransmit(true);
    let initial = Transport::new(initial_config);
    let iface = *initial.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let initial_announce = remote_destination
        .announce(OsRng, None)
        .expect("initial announce");
    let destination = initial_announce.destination;
    handle_announce(
        &initial_announce,
        initial.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert!(initial.has_path(&destination).await);
    assert_eq!(initial.save_reticulum_path_table(temp.path()).await.expect("save"), 1);

    // Expiry at Unix epoch makes the production daemon restore schedule
    // deterministic without changing the transport's production clock.
    let table_path = temp.path().join("destination_table");
    let payload = std::fs::read(&table_path).expect("read persisted routes");
    let mut entries = PathTable::decode_python_entries(&payload).expect("decode persisted routes");
    assert_eq!(entries.len(), 1);
    entries[0].timestamp_secs = 0.0;
    entries[0].expires_secs = 0.0;
    std::fs::write(
        &table_path,
        PathTable::encode_python_entries(&entries).expect("encode expired route"),
    )
    .expect("persist expired route");

    let mut replacement_config = TransportConfig::new("route-restart", &local_identity, true);
    replacement_config.set_retransmit(true);
    let replacement = Transport::new(replacement_config);
    let replacement_iface = *replacement.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(replacement_iface, iface, "replacement uses the persisted interface mapping");
    let report = replacement
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore expired persisted route");
    assert_eq!(report.restored_active_paths, 0);
    assert_eq!(report.skipped.active_expired, 1);
    assert!(!replacement.has_path(&destination).await);
    assert!(replacement.destination_identity(&destination).await.is_none());

    let fresh_announce = remote_destination.announce(OsRng, None).expect("fresh announce");
    assert_eq!(fresh_announce.destination, destination);
    handle_announce(
        &fresh_announce,
        replacement.get_handler().lock().await,
        replacement_iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert!(replacement.has_path(&destination).await, "fresh announce restores route availability");
    assert!(replacement.destination_identity(&destination).await.is_some());
}
