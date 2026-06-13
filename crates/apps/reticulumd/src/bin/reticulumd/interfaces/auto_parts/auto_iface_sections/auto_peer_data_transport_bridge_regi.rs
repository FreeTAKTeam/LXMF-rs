    #[tokio::test]
    async fn auto_peer_data_transport_bridge_registers_virtual_iface_and_routes_direct_tx() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        let sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        let bind_addr = sockets[0].bind_addr;
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let rx_recv = iface_manager.lock().await.receiver();
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let host_iface = channel.address;
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let data_handles = plan.spawn_peer_data_receive_loops(
            sockets,
            Arc::clone(&state),
            dedupe,
            Some(bridge.clone()),
            events_tx,
            shutdown_rx.clone(),
        );
        let tx_handle = plan.spawn_peer_data_transport_tx_loop(bridge, tx_channel, shutdown_rx);
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        state.lock().await.observe_discovery_packet(
            &source_address,
            "lo",
            core::time::Duration::ZERO,
        );
        let inbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x44; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"inbound"),
            ..Default::default()
        };
        let inbound_payload = inbound_packet.to_bytes().expect("serialize inbound packet");

        sender.send_to(&inbound_payload, bind_addr).await.expect("send peer data datagram");
        let processed = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("processed event timeout")
            .expect("processed event");
        assert!(matches!(
            processed,
            AutoPeerDataLoopEvent::Processed(AutoProcessedPeerDataDatagram {
                decision: AutoPeerInboundDecision::Accepted { .. },
                ..
            })
        ));

        let rx_message =
            tokio::time::timeout(std::time::Duration::from_secs(1), rx_recv.lock().await.recv())
                .await
                .expect("rx message timeout")
                .expect("rx message");
        assert_ne!(rx_message.address, host_iface);
        assert_eq!(rx_message.packet, inbound_packet);
        assert_eq!(rx_message.source, IfaceSource::Udp(sender.local_addr().expect("sender addr")));
        assert_eq!(
            iface_manager.lock().await.role(&rx_message.address),
            Some(IfaceRole::VirtualUnicast)
        );

        let outbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x55; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"outbound"),
            ..Default::default()
        };
        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(rx_message.address),
                packet: outbound_packet,
            })
            .await;
        let mut outbound_payload = [0u8; 512];
        let (received, _) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sender.recv_from(&mut outbound_payload),
        )
        .await
        .expect("outbound receive timeout")
        .expect("outbound receive");
        let decoded = Packet::deserialize(&mut InputBuffer::new(&outbound_payload[..received]))
            .expect("decode outbound packet");
        assert_eq!(decoded, outbound_packet);

        shutdown_tx.send(true).expect("send shutdown");
        for handle in data_handles {
            tokio::time::timeout(std::time::Duration::from_secs(1), handle)
                .await
                .expect("peer data loop shutdown timeout")
                .expect("peer data loop task");
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), tx_handle)
            .await
            .expect("tx loop shutdown timeout")
            .expect("tx loop task");
    }

    #[test]
    fn auto_process_discovery_datagram_authenticates_local_echo() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: "eth0".to_string(),
            bind_addr: "[::]:48555".parse().expect("bind addr"),
            multicast_group_addr: Some(
                "[ff0e:0:77b9:4bfd:9488:364b:4bbe:119d]:48555".parse().expect("group addr"),
            ),
            source_addr: "[fe80::1234]:48555".parse().expect("source addr"),
            payload: rns_transport::iface::auto::peering_token(b"field-net", "fe80::1234").to_vec(),
        };

        let processed = plan
            .process_discovery_datagram(&mut state, datagram, core::time::Duration::from_secs(2))
            .expect("authenticated local echo");

        assert_eq!(processed.source_address, "fe80::1234");
        assert_eq!(
            processed.event,
            AutoDiscoveryEvent::LocalMulticastEcho { ifname: "eth0".to_string() }
        );
    }

    #[test]
    fn auto_process_discovery_datagram_authenticates_remote_peer() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: "eth0".to_string(),
            bind_addr: "[fe80::1234]:48556".parse().expect("bind addr"),
            multicast_group_addr: None,
            source_addr: "[fe80::2222]:48556".parse().expect("source addr"),
            payload: rns_transport::iface::auto::peering_token(b"field-net", "fe80::2222").to_vec(),
        };

        let processed = plan
            .process_discovery_datagram(&mut state, datagram, core::time::Duration::from_secs(2))
            .expect("authenticated remote peer");

        assert_eq!(processed.source_address, "fe80::2222");
        assert_eq!(
            processed.event,
            AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
        );
        assert!(state.peer("fe80::2222").is_some());
    }

    #[test]
    fn auto_process_discovery_datagram_rejects_invalid_token() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: "eth0".to_string(),
            bind_addr: "[fe80::1234]:48556".parse().expect("bind addr"),
            multicast_group_addr: None,
            source_addr: "[fe80::2222]:48556".parse().expect("source addr"),
            payload: vec![0; rns_transport::hash::HASH_SIZE],
        };

        let err = plan
            .process_discovery_datagram(&mut state, datagram, core::time::Duration::from_secs(2))
            .expect_err("invalid token should reject");

        assert_eq!(err, AutoDiscoveryRejectReason::InvalidToken);
        assert!(state.peer("fe80::2222").is_none());
    }
