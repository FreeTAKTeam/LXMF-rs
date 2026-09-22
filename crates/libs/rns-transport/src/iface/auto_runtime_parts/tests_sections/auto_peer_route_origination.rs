    // A peer that behaves the way Python's AutoInterface does: it listens on
    // the data port and transmits from an unbound socket, so the source port
    // its datagrams carry is ephemeral and nothing is listening there.
    struct PythonShapedPeer {
        listener: Arc<tokio::net::UdpSocket>,
        sender: tokio::net::UdpSocket,
        data_port: u16,
    }

    impl PythonShapedPeer {
        async fn bind() -> Self {
            let listener = Arc::new(
                tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind peer data listener"),
            );
            let data_port = listener.local_addr().expect("peer data addr").port();
            let sender =
                tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind peer send socket");
            Self { listener, sender, data_port }
        }

        fn ephemeral_source(&self) -> SocketAddr {
            self.sender.local_addr().expect("peer send addr")
        }

        async fn recv(&self) -> Option<Packet> {
            let mut payload = [0u8; 512];
            let (received, _) = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                self.listener.recv_from(&mut payload),
            )
            .await
            .ok()?
            .expect("peer data receive");
            Some(
                Packet::deserialize(&mut InputBuffer::new(&payload[..received]))
                    .expect("decode packet delivered to the peer data port"),
            )
        }
    }

    fn loopback_plan_with_data_port(data_port: u16) -> AutoRuntimePlan {
        let config = AutoInterfaceConfig {
            group_id: "route-origination".to_string(),
            discovery_scope: AutoDiscoveryScope::Global,
            multicast_address_type: MulticastAddressType::Permanent,
            discovery_port: 48_556,
            data_port,
        };
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.2".to_string(),
        };
        AutoRuntimePlan {
            config: config.clone(),
            platform: AutoInterfacePlatform::Other,
            device_filter: AutoInterfaceDeviceFilter::default(),
            candidates: vec![AutoInterfaceDeviceCandidate {
                ifname: "lo".to_string(),
                ipv6_addresses: vec!["127.0.0.2".to_string()],
            }],
            adopted_devices: vec![adopted.clone()],
            peering_packets: vec![config.multicast_peering_packet(&adopted)],
            startup_plan: AutoStartupPlan {
                discovery_listeners: vec![AutoDiscoveryListenerBinding {
                    ifname: "lo".to_string(),
                    link_local_address: "127.0.0.2".to_string(),
                    unicast_bind_address: "127.0.0.1".to_string(),
                    unicast_bind_port: 0,
                    multicast_group_address: "239.255.0.1".to_string(),
                    multicast_bind_address: "239.255.0.1".to_string(),
                    multicast_bind_port: 0,
                }],
                data_listeners: vec![AutoDataListenerBinding {
                    ifname: "lo".to_string(),
                    link_local_address: "127.0.0.2".to_string(),
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 0,
                }],
                peer_job_interval: core::time::Duration::from_millis(100),
                initial_peering_wait: core::time::Duration::ZERO,
            },
        }
    }

    // Admits a peer the way a real peering announce does, so the data path
    // sees a known peer rather than a stranger.
    fn admit_peer(plan: &AutoRuntimePlan, state: &mut AutoDiscoveryState, source: SocketAddr) -> AutoProcessedDiscoveryDatagram {
        let peer_address = source.ip().to_string();
        plan.process_discovery_datagram(
            state,
            AutoDiscoveryDatagram {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "lo".to_string(),
                bind_addr: "127.0.0.1:0".parse().expect("bind addr"),
                multicast_group_addr: None,
                source_addr: source,
                payload: crate::iface::auto::peering_token(
                    plan.config.group_id.as_bytes(),
                    &peer_address,
                )
                .to_vec(),
            },
            core::time::Duration::from_millis(5),
        )
        .expect("authenticated discovery datagram")
        .expect("discovery admits the peer")
    }

    fn test_packet(tag: &[u8], marker: u8) -> Packet {
        Packet {
            destination: AddressHash::new_from_slice(&[marker; crate::hash::ADDRESS_HASH_SIZE]),
            data: crate::packet::PacketDataBuffer::new_from_slice(tag),
            ..Default::default()
        }
    }

    // A peer's data reaches us from whatever source port it happened to send
    // from. Python's outbound socket is never bound, so replying to that
    // source address reaches a port nothing is listening on. The reply has to
    // go to the peer's data port, which is what `peer_data_target` says.
    #[tokio::test]
    async fn auto_peer_reply_goes_to_the_data_port_not_the_senders_source_port() {
        let peer = PythonShapedPeer::bind().await;
        let plan = loopback_plan_with_data_port(peer.data_port);
        let mut state = plan.discovery_state();
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let rx_recv = iface_manager.lock().await.receiver();
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let _tx_handle =
            plan.spawn_peer_data_transport_tx_loop(bridge.clone(), tx_channel, shutdown_rx);
        let our_data_socket = Arc::new(
            tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind our data socket"),
        );
        let our_bind_addr = our_data_socket.local_addr().expect("our data addr");

        admit_peer(&plan, &mut state, peer.ephemeral_source());

        let inbound = test_packet(b"peer-to-us", 0x33);
        let processed = plan
            .process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                AutoPeerDataDatagram {
                    ifname: "lo".to_string(),
                    bind_addr: our_bind_addr,
                    source_addr: peer.ephemeral_source(),
                    payload: inbound.to_bytes().expect("serialize inbound"),
                },
                core::time::Duration::from_millis(10),
            )
            .expect("peer data admitted");
        assert!(matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }));
        assert_eq!(
            bridge.forward_peer_data(&processed, Arc::clone(&our_data_socket), peer.data_port).await,
            AutoPeerDataForwardResult::Delivered
        );

        let rx_message =
            tokio::time::timeout(std::time::Duration::from_secs(1), rx_recv.lock().await.recv())
                .await
                .expect("transport rx timeout")
                .expect("transport rx");
        let virtual_iface = rx_message.address;

        let outbound = test_packet(b"us-to-peer", 0x44);
        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(virtual_iface),
                packet: outbound.clone(),
            })
            .await;

        assert_eq!(
            peer.recv().await.as_ref(),
            Some(&outbound),
            "the reply must arrive on the peer's data port, not the port it sent from"
        );
    }

    // Python spawns a transmit-capable interface the moment a peering announce
    // authenticates, before any data is exchanged. Discovery alone therefore
    // has to give the transport somewhere to send, or our announces never
    // leave and the peer never learns we are here.
    #[tokio::test]
    async fn auto_discovery_alone_gives_the_transport_a_route_to_the_peer() {
        let peer = PythonShapedPeer::bind().await;
        let plan = loopback_plan_with_data_port(peer.data_port);
        let mut state = plan.discovery_state();

        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let _tx_handle =
            plan.spawn_peer_data_transport_tx_loop(bridge.clone(), tx_channel, shutdown_rx);
        // Drive the same seam the runtime's supervisor task drives, so the
        // socket lookup and the event filtering are covered, not just the
        // bridge call underneath them.
        let shared_state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (data_events_tx, _data_events_rx) = tokio::sync::mpsc::channel(8);
        let (_data_shutdown_tx, data_shutdown_rx) = tokio::sync::watch::channel(false);
        let data_supervisor = Arc::new(tokio::sync::Mutex::new(
            AutoPeerDataListenerSupervisor::new(
                plan.clone(),
                Arc::clone(&shared_state),
                dedupe,
                Some(bridge.clone()),
                data_shutdown_rx,
            ),
        ));
        let data_sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        data_supervisor.lock().await.spawn_sockets(data_sockets, &data_events_tx);

        let admitted = admit_peer(&plan, &mut state, peer.ephemeral_source());
        assert_eq!(
            admitted.event,
            AutoDiscoveryEvent::Peer(crate::iface::auto::AutoPeerEvent::Added)
        );

        register_discovered_peer_route(
            &AutoDiscoveryLoopEvent::Processed(admitted),
            &Some(bridge.clone()),
            &data_supervisor,
            &plan.config,
        )
        .await;

        // An announce is a broadcast. With no route it iterates an empty map
        // and returns normally, which is the silent half of this defect.
        let announce = test_packet(b"our-announce", 0x55);
        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Broadcast(None),
                packet: announce.clone(),
            })
            .await;

        assert_eq!(
            peer.recv().await.as_ref(),
            Some(&announce),
            "a peer known only from discovery must still be reachable"
        );
    }
