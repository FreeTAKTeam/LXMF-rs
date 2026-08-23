use super::inbound_processing::preprocess_inbound_message;

#[tokio::test]
async fn rns_1_5_originated_link_request_signals_next_hop_interface_mtu() {
    const NEXT_HOP_MTU: usize = 500;
    const LINK_MTU_MASK: u32 = 0x1f_ffff;

    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, true));
    let mut iface_channel = transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role_mode_mtu(
            8,
            IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            NEXT_HOP_MTU,
    );
    let iface = *iface_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_signing_key = remote_identity.sign_key().clone();
    let destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_desc = destination.desc;
    {
        let handler = transport.get_handler();
        assert!(handler.lock().await.path_table.restore_tunnel_path(
            destination_desc.address_hash,
            destination_desc.address_hash,
            1,
            iface,
            Hash::new_from_slice(b"next-hop-path"),
            std::time::Instant::now(),
        ));
    }

    let link = transport.link(destination_desc).await;
    let request = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("link request should be queued")
        .expect("interface channel open");
    let signalling = &request.packet.data.as_slice()[request.packet.data.len() - 3..];
    let value = ((signalling[0] as u32) << 16)
        | ((signalling[1] as u32) << 8)
        | signalling[2] as u32;
    assert_eq!((value & LINK_MTU_MASK) as usize, NEXT_HOP_MTU);

    let (event_tx, _) = tokio::sync::broadcast::channel(4);
    let mut responder = Link::new_from_request(
        &request.packet,
        remote_signing_key,
        destination_desc,
        event_tx,
    )
    .expect("small-MTU link request should parse");
    assert!(matches!(
        link.lock().await.handle_packet(&responder.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
}

#[test]
fn rns_1_5_direct_link_channel_rejects_u16_length_overflow() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let identity = *remote_identity.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (event_tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination, event_tx.clone());
    let request = outbound.request();
    let mut responder = Link::new_from_request(
        &request,
        remote_identity.sign_key().clone(),
        destination,
        event_tx,
    )
    .expect("link request should parse");
    assert!(matches!(
        outbound.handle_packet(&responder.prove(), AddressHash::new_from_rand(OsRng)),
        crate::destination::link::LinkHandleResult::Activated
    ));
    assert!(matches!(
        outbound.send_channel_message(0x4411, vec![0xAA; u16::MAX as usize + 1]),
        Err(ChannelError::PayloadTooLarge)
    ));
}

#[tokio::test]
async fn rns_1_5_public_path_request_records_outbound_interface_telemetry() {
    let transport = Transport::new(TransportConfig::default());
    let channel = transport.iface_manager().lock().await.new_channel(8);
    let iface = *channel.address();
    let destination = AddressHash::new_from_slice(&[0x71; crate::hash::ADDRESS_HASH_SIZE]);
    let dispatch = transport.request_path(&destination, Some(iface), None).await;
    assert_eq!(dispatch.sent_ifaces, 1);
    let snapshot = transport
        .interface_traffic_snapshots()
        .await
        .into_iter()
        .find(|snapshot| snapshot.address == iface)
        .expect("interface snapshot");
    assert_eq!(snapshot.path_request_tx_count, 1);
    assert!(snapshot.path_request_tx_bytes > 0);
}

#[tokio::test]
async fn rns_1_5_ingress_records_live_ifac_flag_policy_violations() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let authenticated = crate::iface::RxMessage {
        address: iface,
        packet: Packet {
            header: Header {
                ifac_flag: crate::packet::IfacFlag::Authenticated,
                ..Default::default()
            },
            destination: AddressHash::new_from_slice(&[0x61; crate::hash::ADDRESS_HASH_SIZE]),
            ..Default::default()
        },
        source: Default::default(),
    };
    assert!(
        preprocess_inbound_message(&handler, &transport.iface_messages_tx, authenticated)
            .await
            .is_none()
    );

    assert!(transport.iface_manager().lock().await.set_shared_config(
        iface,
        crate::iface::InterfaceSharedConfig {
            network_name: Some("field-net".to_string()),
            ..Default::default()
        },
    ));
    let missing = crate::iface::RxMessage {
        address: iface,
        packet: Packet {
            destination: AddressHash::new_from_slice(&[0x62; crate::hash::ADDRESS_HASH_SIZE]),
            ..Default::default()
        },
        source: Default::default(),
    };
    assert!(
        preprocess_inbound_message(&handler, &transport.iface_messages_tx, missing)
            .await
            .is_none()
    );
    let snapshot = transport
        .iface_manager()
        .lock()
        .await
        .traffic_snapshots()
        .into_iter()
        .find(|snapshot| snapshot.address == iface)
        .expect("interface snapshot");
    assert_eq!(snapshot.ifac_violations, 2);
}

#[tokio::test]
async fn rns_1_5_parent_interface_reports_active_child_burst_counts() {
    let transport = Transport::new(TransportConfig::default());
    let (parent, child) = {
        let interface_manager = transport.iface_manager();
        let mut interfaces = interface_manager.lock().await;
        let parent = *interfaces.new_channel(8).address();
        let child = *interfaces.new_channel(8).address();
        assert!(interfaces.inherit_runtime_config(parent, child));
        assert!(interfaces.set_shared_config(
            child,
            crate::iface::InterfaceSharedConfig {
                ingress_control: Some(true),
                ic_pr_burst_freq_new: Some(0.0),
                ..Default::default()
            },
        ));
        for _ in 0..3 {
            assert!(interfaces.record_inbound_traffic(child, PacketType::Data, true, 32));
        }
        assert!(interfaces.should_ingress_limit_path_request(child));
        (parent, child)
    };
    let announce = Packet {
        header: Header { packet_type: PacketType::Announce, ..Default::default() },
        destination: AddressHash::new_from_slice(&[0x51; crate::hash::ADDRESS_HASH_SIZE]),
        ..Default::default()
    };
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        let shared = crate::iface::InterfaceSharedConfig {
            ingress_control: Some(true),
            ic_burst_freq_new: Some(0.0),
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = handler.announce_limits.check_with_shared_config(
                child,
                &announce,
                crate::iface::IfaceSource::None,
                false,
                &shared,
            );
        }
    }

    let snapshots = transport.interface_traffic_snapshots().await;
    let parent = snapshots
        .iter()
        .find(|snapshot| snapshot.address == parent)
        .expect("parent snapshot");
    assert_eq!(parent.ic_burst_count, Some(1));
    assert_eq!(parent.ic_pr_burst_count, Some(1));
    assert!(parent.announce_burst_active);
    assert!(parent.path_request_burst_active);
    assert_eq!(parent.rx_bytes, 96);
    let child = snapshots
        .iter()
        .find(|snapshot| snapshot.address == child)
        .expect("child snapshot");
    assert!(child.announce_burst_active);
    assert!(child.path_request_burst_active);
}

#[tokio::test]
async fn rns_1_5_ingress_rejects_tagless_path_requests_before_queueing() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let path_destination = handler.lock().await.fixed_dest_path_requests;
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let inbound = crate::iface::RxMessage {
        address: iface,
        packet: Packet {
            header: Header { packet_type: PacketType::Data, ..Default::default() },
            destination: path_destination,
            data: PacketDataBuffer::new_from_slice(&[0x22; crate::hash::ADDRESS_HASH_SIZE]),
            ..Default::default()
        },
        source: crate::iface::IfaceSource::None,
    };

    assert!(
        preprocess_inbound_message(&handler, &transport.iface_messages_tx, inbound)
            .await
            .is_none()
    );
    let violations = transport
        .iface_manager()
        .lock()
        .await
        .traffic_snapshots()
        .into_iter()
        .find(|snapshot| snapshot.address == iface)
        .expect("interface snapshot")
        .protocol_violations;
    assert_eq!(violations, 1);
}

#[tokio::test]
async fn rns_1_5_ingress_limits_path_request_bursts_before_queueing() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let channel = transport.iface_manager().lock().await.new_channel_with_role_and_mode(
        8,
        IfaceRole::Unicast,
        crate::iface::InterfaceMode::AccessPoint,
    );
    let iface = *channel.address();
    assert!(transport.iface_manager().lock().await.set_shared_config(
        iface,
        crate::iface::InterfaceSharedConfig {
            ingress_control: Some(true),
            ic_pr_burst_freq_new: Some(0.01),
            ..Default::default()
        },
    ));
    let mut generator = PathRequests::new("", None, 16, 16, 30);

    for index in 0..3 {
        let packet = generator.generate(
            &AddressHash::new_from_slice(&[index + 1; crate::hash::ADDRESS_HASH_SIZE]),
            Some(vec![index + 10; crate::hash::ADDRESS_HASH_SIZE]),
        );
        let (class, _) = preprocess_inbound_message(
            &handler,
            &transport.iface_messages_tx,
            crate::iface::RxMessage {
                address: iface,
                packet,
                source: crate::iface::IfaceSource::None,
            },
        )
        .await
        .expect("unique path request");
        if index < 2 {
            assert_eq!(class, InboundTrafficClass::PathRequest);
        } else {
            assert_eq!(class, InboundTrafficClass::IngressLimited);
        }
    }
}

#[tokio::test]
async fn rns_1_5_ingress_full_path_queue_does_not_poison_retry() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let channel = transport.iface_manager().lock().await.new_channel_with_role_and_mode(
        8,
        IfaceRole::Unicast,
        crate::iface::InterfaceMode::AccessPoint,
    );
    let iface = *channel.address();
    let queues = InboundQueues::new(InboundQueueLimits {
        data: 1,
        announce: 1,
        path_request: 1,
        ingress_limited: 1,
    });
    let mut generator = PathRequests::new("", None, 16, 16, 30);
    let filler = generator.generate(
        &AddressHash::new_from_slice(&[0x31; crate::hash::ADDRESS_HASH_SIZE]),
        Some(vec![0x41; crate::hash::ADDRESS_HASH_SIZE]),
    );
    let retry = generator.generate(
        &AddressHash::new_from_slice(&[0x32; crate::hash::ADDRESS_HASH_SIZE]),
        Some(vec![0x42; crate::hash::ADDRESS_HASH_SIZE]),
    );
    let first = preprocess_inbound_message(
        &handler,
        &transport.iface_messages_tx,
        crate::iface::RxMessage { address: iface, packet: filler, source: Default::default() },
    )
    .await
    .expect("filler request");
    queues.enqueue(first.0, first.1).expect("fill path queue");
    let rejected = preprocess_inbound_message(
        &handler,
        &transport.iface_messages_tx,
        crate::iface::RxMessage {
            address: iface,
            packet: retry.clone(),
            source: Default::default(),
        },
    )
    .await
    .expect("first retry admission");
    let full = queues.enqueue(rejected.0, rejected.1).expect_err("path queue full");
    super::inbound_processing::rollback_rejected_inbound(&handler, &full.item).await;

    assert!(
        preprocess_inbound_message(
            &handler,
            &transport.iface_messages_tx,
            crate::iface::RxMessage { address: iface, packet: retry, source: Default::default() },
        )
        .await
        .is_some(),
        "queue-full rollback must release tag and in-flight state"
    );
}

#[tokio::test]
async fn rns_1_5_invalid_transported_path_request_does_not_poison_valid_retry() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let channel = transport.iface_manager().lock().await.new_channel_with_role_and_mode(
        8,
        IfaceRole::Unicast,
        crate::iface::InterfaceMode::AccessPoint,
    );
    let iface = *channel.address();
    let mut generator = PathRequests::new("", None, 16, 16, 30);
    let mut request = generator.generate(
        &AddressHash::new_from_slice(&[0x73; crate::hash::ADDRESS_HASH_SIZE]),
        Some(vec![0x74; crate::hash::ADDRESS_HASH_SIZE]),
    );
    request.header.hops = 2;

    assert!(
        preprocess_inbound_message(
            &handler,
            &transport.iface_messages_tx,
            crate::iface::RxMessage {
                address: iface,
                packet: request.clone(),
                source: Default::default(),
            },
        )
        .await
        .is_none()
    );

    request.header.hops = 0;
    assert!(
        preprocess_inbound_message(
            &handler,
            &transport.iface_messages_tx,
            crate::iface::RxMessage { address: iface, packet: request, source: Default::default() },
        )
        .await
        .is_some(),
        "invalid transported request must not reserve duplicate or in-flight state"
    );
}
