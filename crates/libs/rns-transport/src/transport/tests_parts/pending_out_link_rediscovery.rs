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

#[tokio::test]
async fn timed_out_routed_link_marks_one_hop_path_unresponsive_and_requests_rediscovery() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("transport-instance", &local_identity, true);
    config.set_retransmit(true);
    config.set_link_proof_timeout_secs(0);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (mut blocked_channel, mut alternate_channel) = {
        let iface_manager = transport.iface_manager();
        let mut ifaces = iface_manager.lock().await;
        (
            ifaces.new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            ),
            ifaces.new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            ),
        )
    };
    let blocked_iface = *blocked_channel.address();
    let alternate_iface = *alternate_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;
    let next_hop = AddressHash::new_from_rand(OsRng);

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            next_hop,
            1,
            blocked_iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let (link_events, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(destination.desc, link_events);
    let mut request = outbound_link.request();
    request.header.hops = 2;
    handle_link_request_as_intermediate(
        blocked_iface,
        next_hop,
        blocked_iface,
        &request,
        handler.lock().await,
    )
    .await;
    let _forwarded = timeout(Duration::from_millis(200), blocked_channel.tx_channel.recv())
        .await
        .expect("link request should forward to the current next-hop iface")
        .expect("tx channel open");

    tokio::time::sleep(Duration::from_millis(1)).await;
    super::jobs::handle_link_table_cleanup(handler.lock().await).await;

    assert!(handler.lock().await.path_table.path_is_unresponsive(&destination_hash));
    assert!(
        blocked_channel.tx_channel.try_recv().is_err(),
        "rediscovery should block the iface that delivered the timed-out routed link request"
    );
    let rediscovery = timeout(Duration::from_millis(200), alternate_channel.tx_channel.recv())
        .await
        .expect("rediscovery path request should be sent on alternate ifaces")
        .expect("tx channel open");
    assert_eq!(rediscovery.tx_type, crate::iface::TxMessageType::Broadcast(Some(blocked_iface)));
    assert_eq!(&rediscovery.packet.data.as_slice()[..ADDRESS_HASH_SIZE], destination_hash.as_slice());
    assert_ne!(blocked_iface, alternate_iface);
}

#[tokio::test]
async fn timed_out_routed_link_expires_one_hop_path_for_non_transport_boundary_ingress() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("client-instance", &local_identity, true);
    config.set_link_proof_timeout_secs(0);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (mut blocked_channel, mut alternate_channel) = {
        let iface_manager = transport.iface_manager();
        let mut ifaces = iface_manager.lock().await;
        (
            ifaces.new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Boundary,
            ),
            ifaces.new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            ),
        )
    };
    let blocked_iface = *blocked_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;
    let next_hop = AddressHash::new_from_rand(OsRng);

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            next_hop,
            1,
            blocked_iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let (link_events, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(destination.desc, link_events);
    let mut request = outbound_link.request();
    request.header.hops = 2;
    handle_link_request_as_intermediate(
        blocked_iface,
        next_hop,
        blocked_iface,
        &request,
        handler.lock().await,
    )
    .await;
    let _forwarded = timeout(Duration::from_millis(200), blocked_channel.tx_channel.recv())
        .await
        .expect("link request should forward to the current next-hop iface")
        .expect("tx channel open");

    tokio::time::sleep(Duration::from_millis(1)).await;
    super::jobs::handle_link_table_cleanup(handler.lock().await).await;

    assert!(handler.lock().await.path_table.get(&destination_hash).is_none());
    assert!(blocked_channel.tx_channel.try_recv().is_err());
    let rediscovery = timeout(Duration::from_millis(200), alternate_channel.tx_channel.recv())
        .await
        .expect("rediscovery path request should be sent on alternate ifaces")
        .expect("tx channel open");
    assert_eq!(rediscovery.tx_type, crate::iface::TxMessageType::Broadcast(Some(blocked_iface)));
    assert_eq!(&rediscovery.packet.data.as_slice()[..ADDRESS_HASH_SIZE], destination_hash.as_slice());
}

#[tokio::test]
async fn timed_out_routed_links_dedupe_path_requests_by_destination_and_keep_blocked_iface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("transport-instance", &local_identity, true);
    config.set_retransmit(true);
    config.set_link_proof_timeout_secs(0);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (mut blocked_channel, mut alternate_channel) = {
        let iface_manager = transport.iface_manager();
        let mut ifaces = iface_manager.lock().await;
        (
            ifaces.new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            ),
            ifaces.new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            ),
        )
    };
    let blocked_iface = *blocked_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = destination.desc.address_hash;
    let next_hop = AddressHash::new_from_rand(OsRng);

    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            next_hop,
            1,
            blocked_iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let (first_events, _) = tokio::sync::broadcast::channel(4);
    let (second_events, _) = tokio::sync::broadcast::channel(4);
    let mut local_branch_link = Link::new(destination.desc, first_events);
    let mut blocked_branch_link = Link::new(destination.desc, second_events);
    let local_branch_request = local_branch_link.request();
    let mut blocked_branch_request = blocked_branch_link.request();
    blocked_branch_request.header.hops = 2;

    for request in [&local_branch_request, &blocked_branch_request] {
        handle_link_request_as_intermediate(
            blocked_iface,
            next_hop,
            blocked_iface,
            request,
            handler.lock().await,
        )
        .await;
        let _forwarded = timeout(Duration::from_millis(200), blocked_channel.tx_channel.recv())
            .await
            .expect("link request should forward to the current next-hop iface")
            .expect("tx channel open");
    }

    tokio::time::sleep(Duration::from_millis(1)).await;
    super::jobs::handle_link_table_cleanup(handler.lock().await).await;

    assert!(blocked_channel.tx_channel.try_recv().is_err());
    let rediscovery = timeout(Duration::from_millis(200), alternate_channel.tx_channel.recv())
        .await
        .expect("one blocked-interface rediscovery path request should be queued")
        .expect("tx channel open");
    assert_eq!(rediscovery.tx_type, crate::iface::TxMessageType::Broadcast(Some(blocked_iface)));
    assert_eq!(&rediscovery.packet.data.as_slice()[..ADDRESS_HASH_SIZE], destination_hash.as_slice());
    assert!(alternate_channel.tx_channel.try_recv().is_err());
}
