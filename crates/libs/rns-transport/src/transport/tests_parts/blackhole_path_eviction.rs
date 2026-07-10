#[tokio::test]
async fn blackholed_identity_path_eviction_removes_only_associated_destinations() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let iface = AddressHash::new_from_rand(OsRng);
    let other_iface = AddressHash::new_from_rand(OsRng);

    let blackholed_identity = PrivateIdentity::new_from_rand(OsRng);
    let blackholed_identity_hash = *blackholed_identity.address_hash();
    let mut first_destination = SingleInputDestination::new(
        blackholed_identity,
        DestinationName::new("lxmf", "delivery"),
    );
    let other_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut other_destination = SingleInputDestination::new(
        other_identity,
        DestinationName::new("lxmf", "delivery"),
    );
    let first_announce = first_destination.announce(OsRng, None).expect("first announce");
    let other_announce = other_destination.announce(OsRng, None).expect("other announce");

    handle_announce(
        &first_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    handle_announce(
        &other_announce,
        handler.lock().await,
        other_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert!(transport.has_path(&first_announce.destination).await);
    assert!(transport.has_path(&other_announce.destination).await);

    assert_eq!(transport.expire_paths_for_identity(&blackholed_identity_hash).await, 1);
    assert!(!transport.has_path(&first_announce.destination).await);
    assert!(transport.has_path(&other_announce.destination).await);
    assert_eq!(transport.expire_paths_for_identity(&blackholed_identity_hash).await, 0);
}
