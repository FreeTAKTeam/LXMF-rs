#[tokio::test]
async fn shared_instance_transport_wraps_one_hop_outbound_packets() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-instance-client", &local_identity, true);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();
    let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));

    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        assert!(handler.path_table.restore_tunnel_path(
            destination,
            destination,
            1,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let packet = Packet {
        header: Header {
            header_type: HeaderType::Type1,
            propagation_type: crate::packet::PropagationType::Broadcast,
            destination_type: DestinationType::Single,
            packet_type: PacketType::LinkRequest,
            ..Header::default()
        },
        destination,
        data: PacketDataBuffer::new_from_slice(b"link request"),
        ..Packet::default()
    };

    let trace = transport.send_packet_with_trace(packet).await;
    assert_eq!(trace.outcome, SendPacketOutcome::SentDirect);
    assert_eq!(trace.direct_iface, Some(iface));

    let sent = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("shared-instance packet should be queued")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, crate::iface::TxMessageType::Direct(iface));
    assert_eq!(sent.packet.header.header_type, HeaderType::Type2);
    assert_eq!(sent.packet.header.propagation_type, crate::packet::PropagationType::Transport);
    assert_eq!(sent.packet.transport, Some(destination));
}

#[tokio::test]
async fn unknown_announces_are_held_per_interface_and_released_by_lowest_hops() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = AddressHash::new_from_rand(OsRng);

    let mut first_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut first_announce = first_destination.announce(OsRng, None).expect("announce");
    first_announce.header.hops = 4;
    handle_announce(&first_announce, handler.lock().await, iface, crate::iface::IfaceSource::None)
        .await;
    let first_event = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");
    assert_eq!(first_event.hops, 4);
    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut higher_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut higher_hop_announce = higher_hop_destination.announce(OsRng, None).expect("announce");
    higher_hop_announce.header.hops = 3;
    handle_announce(
        &higher_hop_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut lower_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut lower_hop_announce = lower_hop_destination.announce(OsRng, None).expect("announce");
    lower_hop_announce.header.hops = 1;
    handle_announce(
        &lower_hop_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let mut immediate_hops = Vec::new();
    while let Ok(event) = announce_rx.try_recv() {
        immediate_hops.push(event.hops);
    }
    assert!(
        immediate_hops.iter().all(|hops| matches!(*hops, 1 | 3)),
        "unexpected immediate announce release sequence {immediate_hops:?}"
    );
    if let Some(hops) = immediate_hops.first().copied() {
        assert_eq!(hops, 3);
    }

    tokio::time::sleep(Duration::from_millis(80)).await;
    if immediate_hops.contains(&1) {
        release_held_announces(handler.lock().await).await;
        assert!(matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    } else {
        let mut released_lowest = None;
        for _ in 0..4 {
            release_held_announces(handler.lock().await).await;
            if let Ok(event) = timeout(Duration::from_millis(120), announce_rx.recv()).await {
                released_lowest = Some(event.expect("broadcast receive"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let released_lowest = released_lowest.expect("lowest-hop held announce should emit");
        assert_eq!(released_lowest.hops, 1);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    release_held_announces(handler.lock().await).await;

    if immediate_hops.contains(&3) {
        assert!(matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    } else {
        tokio::time::sleep(Duration::from_millis(25)).await;
        release_held_announces(handler.lock().await).await;
        let released_next = timeout(Duration::from_millis(200), announce_rx.recv())
            .await
            .expect("next held announce should emit")
            .expect("broadcast receive");
        assert_eq!(released_next.hops, 3);
    }
}

#[tokio::test]
async fn ingress_control_false_bypasses_unknown_announce_holding() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 0.1,
        burst_freq: 0.1,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let iface = *manager.new_channel(8).address();
        manager.set_shared_config(
            iface,
            crate::iface::InterfaceSharedConfig {
                ingress_control: Some(false),
                ..Default::default()
            },
        );
        iface
    };

    let mut first_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let first_announce = first_destination.announce(OsRng, None).expect("first announce");
    handle_announce(&first_announce, handler.lock().await, iface, crate::iface::IfaceSource::None)
        .await;
    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");

    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut second_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let second_announce = second_destination.announce(OsRng, None).expect("second announce");
    handle_announce(&second_announce, handler.lock().await, iface, crate::iface::IfaceSource::None)
        .await;

    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("second announce should bypass limiter and emit")
        .expect("broadcast receive");
}
