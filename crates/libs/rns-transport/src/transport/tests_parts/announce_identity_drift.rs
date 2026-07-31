// Issue #517: an announce for an already-known destination hash carrying a
// different key pair than the one on record must be rejected as identity
// drift (reference `Identity.validate_announce` parity), while a rotated
// app_data payload from the SAME key pair must still be accepted.

#[tokio::test]
async fn announce_with_drifted_identity_is_rejected_without_overwrite() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;

    // Simulate a desynchronized/poisoned identity cache: an entry for the
    // real destination hash carrying an unrelated key pair.
    let impostor_identity = *PrivateIdentity::new_from_rand(OsRng).as_identity();
    let impostor_destination = crate::destination::new_out(impostor_identity, "lxmf", "delivery");
    {
        let mut guard = handler.lock().await;
        guard
            .single_out_destinations
            .insert(destination, Arc::new(Mutex::new(impostor_destination)));
    }

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    assert!(
        matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "drifted announce must not emit an announce event"
    );

    let guard = handler.lock().await;
    let stored = guard.single_out_destinations.get(&destination).expect("entry").clone();
    let stored = stored.lock().await;
    assert_eq!(
        stored.identity.public_key_bytes(),
        impostor_identity.public_key_bytes(),
        "drifted announce must not overwrite the stored identity"
    );
    drop(stored);
    assert!(
        guard.path_table.get(&destination).is_none(),
        "drifted announce must not install a path entry"
    );
}

#[tokio::test]
async fn reannounce_with_rotated_app_data_is_accepted() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let first = remote_destination
        .announce(OsRng, Some(b"stamp-cost-16"))
        .expect("first announce");

    handle_announce(&first, handler.lock().await, iface, crate::iface::IfaceSource::None).await;
    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");

    // Same key pair, different app_data (e.g. rotated LXMF stamp cost):
    // reference Reticulum overwrites app_data per accepted announce, so
    // this must NOT be treated as identity drift. Wait past the ingress
    // hold window so the reannounce is processed, not rate-held.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let second = remote_destination
        .announce(OsRng, Some(b"stamp-cost-22"))
        .expect("rotated app_data announce");
    handle_announce(&second, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let received = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("app_data rotation from the same identity must still be accepted")
        .expect("broadcast receive");
    assert_eq!(received.app_data.as_slice(), b"stamp-cost-22");
}
