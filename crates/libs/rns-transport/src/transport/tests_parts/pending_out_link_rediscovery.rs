use crate::hash::ADDRESS_HASH_SIZE;

#[tokio::test]
async fn closed_pending_out_link_expires_path_and_requests_rediscovery_like_python() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    link.lock().await.close();

    super::jobs::handle_check_links(handler.lock().await).await;

    assert!(handler.lock().await.path_table.get(&destination_hash).is_none());
    let rediscovery = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("rediscovery path request should be queued")
        .expect("tx channel open");
    assert_eq!(rediscovery.tx_type, crate::iface::TxMessageType::Broadcast(None));
    assert_eq!(&rediscovery.packet.data.as_slice()[..ADDRESS_HASH_SIZE], destination_hash.as_slice());
}

#[tokio::test]
async fn shared_instance_closed_pending_out_link_expires_path_without_local_rediscovery() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-client", &local_identity, true);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    link.lock().await.close();

    super::jobs::handle_check_links(handler.lock().await).await;

    assert!(handler.lock().await.path_table.get(&destination_hash).is_none());
    assert!(iface_channel.tx_channel.try_recv().is_err());
}

#[tokio::test]
async fn transport_instance_closed_pending_out_link_keeps_path_without_rediscovery() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("transport-instance", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    link.lock().await.close();

    super::jobs::handle_check_links(handler.lock().await).await;

    assert!(handler.lock().await.path_table.get(&destination_hash).is_some());
    assert!(iface_channel.tx_channel.try_recv().is_err());
}

#[tokio::test]
async fn closed_pending_out_link_rediscovery_respects_recent_path_request_throttle() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
        guard.path_requests.record_outgoing_request(&destination_hash);
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    link.lock().await.close();

    super::jobs::handle_check_links(handler.lock().await).await;

    assert!(handler.lock().await.path_table.get(&destination_hash).is_none());
    assert!(iface_channel.tx_channel.try_recv().is_err());
}

#[tokio::test]
async fn aged_pending_out_link_repeats_without_rediscovery_until_closed_like_python() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            destination_hash,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination.desc).await;
    let _initial_request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("initial link request should be queued")
        .expect("tx channel open");
    link.lock()
        .await
        .set_request_time_for_test(std::time::Instant::now() - Duration::from_secs(7));

    super::jobs::handle_check_links(handler.lock().await).await;

    assert!(handler.lock().await.path_table.get(&destination_hash).is_some());
    let repeated = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("repeated link request should be queued")
        .expect("tx channel open");
    assert_eq!(repeated.packet.header.packet_type, PacketType::LinkRequest);
    assert!(iface_channel.tx_channel.try_recv().is_err());
}
