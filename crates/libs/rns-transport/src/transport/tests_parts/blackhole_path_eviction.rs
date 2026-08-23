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

    assert_eq!(
        transport.set_identity_blackholed(blackholed_identity_hash, true).await,
        1
    );
    assert!(transport.is_identity_blackholed(&blackholed_identity_hash).await);
    assert!(!transport.has_path(&first_announce.destination).await);
    assert!(transport.has_path(&other_announce.destination).await);
    assert_eq!(transport.expire_paths_for_identity(&blackholed_identity_hash).await, 0);

    handle_announce(
        &first_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert!(
        !transport.has_path(&first_announce.destination).await,
        "blackholed identity announce must expose a distinct filtered outcome"
    );

    transport.set_identity_blackholed(blackholed_identity_hash, false).await;
    assert!(!transport.is_identity_blackholed(&blackholed_identity_hash).await);
    handle_announce(
        &first_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert!(transport.has_path(&first_announce.destination).await);
}

#[tokio::test]
async fn rns_1_5_expiring_transport_blackhole_stops_filtering() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("blackhole-expiry", &local_identity, true));
    let identity = AddressHash::new_from_slice(&[0x44; 16]);
    transport.set_identity_blackholed_until(identity, true, Some(1.0)).await;
    assert!(!transport.is_identity_blackholed(&identity).await);
}
