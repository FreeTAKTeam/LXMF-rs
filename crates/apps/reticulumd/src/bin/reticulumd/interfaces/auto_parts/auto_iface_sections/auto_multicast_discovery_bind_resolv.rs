    #[test]
    fn auto_multicast_discovery_bind_resolves_link_scope_group_to_unspecified_bind() {
        let target = AutoDiscoverySocketBindTarget {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: "eth0".to_string(),
            bind_host: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            bind_port: 29_716,
            scope_ifname: Some("eth0".to_string()),
            multicast_group_host: Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string()),
        };

        let resolved = target
            .resolve_multicast_bind(|ifname| {
                assert_eq!(ifname, "eth0");
                Ok(7)
            })
            .expect("resolve multicast bind");

        assert_eq!(resolved.bind_addr.to_string(), "[::]:29716");
        assert_eq!(
            resolved.multicast_group_addr.to_string(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%7]:29716"
        );
        assert_eq!(resolved.multicast_scope_id, 7);
    }

    #[test]
    fn auto_multicast_discovery_bind_uses_ifname_scope_for_windows_empty_host() {
        let target = AutoDiscoverySocketBindTarget {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: "Ethernet".to_string(),
            bind_host: "::".to_string(),
            bind_port: 29_716,
            scope_ifname: None,
            multicast_group_host: Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string()),
        };

        let resolved = target
            .resolve_multicast_bind(|ifname| {
                assert_eq!(ifname, "Ethernet");
                Ok(11)
            })
            .expect("resolve multicast bind");

        assert_eq!(resolved.bind_addr.to_string(), "[::]:29716");
        assert_eq!(
            resolved.multicast_group_addr.to_string(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%11]:29716"
        );
        assert_eq!(resolved.multicast_scope_id, 11);
    }

    #[test]
    fn auto_peer_announce_datagram_formats_socket_targets_for_ipv6_multicast() {
        let link_plan = build_startup_plan_from_candidates(
            &default_link_auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("link startup plan");
        let link_datagram = link_plan.initial_peer_announce_datagrams().remove(0);
        assert_eq!(
            link_datagram.destination_socket_target(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0]:29716"
        );
        assert_eq!(
            link_datagram.socket_target(),
            AutoPeerAnnounceSocketTarget {
                host: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
                port: 29_716,
                scope_ifname: Some("eth0".to_string()),
            }
        );

        let global_plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("global startup plan");
        let global_datagram = global_plan.initial_peer_announce_datagrams().remove(0);
        assert_eq!(
            global_datagram.destination_socket_target(),
            "[ff0e:0:77b9:4bfd:9488:364b:4bbe:119d]:48555"
        );
        assert_eq!(
            global_datagram.socket_target(),
            AutoPeerAnnounceSocketTarget {
                host: "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d".to_string(),
                port: 48_555,
                scope_ifname: None,
            }
        );
    }

    #[test]
    fn auto_peer_announce_socket_target_preserves_explicit_scope() {
        let datagram = AutoPeerAnnounceDatagram {
            kind: AutoPeeringPacketKind::ReverseUnicast,
            ifname: "wlan0".to_string(),
            source_link_local_address: "fe80::1111".to_string(),
            destination_address: "fe80::2222%wlan0".to_string(),
            destination_port: 29_717,
            payload: vec![0; rns_transport::hash::HASH_SIZE],
        };

        assert_eq!(
            datagram.socket_target(),
            AutoPeerAnnounceSocketTarget {
                host: "fe80::2222".to_string(),
                port: 29_717,
                scope_ifname: Some("wlan0".to_string()),
            }
        );
        assert_eq!(datagram.destination_socket_target(), "[fe80::2222%wlan0]:29717");
    }

    #[test]
    fn auto_peer_announce_socket_target_resolves_scoped_ipv6() {
        let target = AutoPeerAnnounceSocketTarget {
            host: "fe80::2222".to_string(),
            port: 29_717,
            scope_ifname: Some("eth0".to_string()),
        };

        let resolved = target
            .resolve_socket_addr(|ifname| {
                assert_eq!(ifname, "eth0");
                Ok(7)
            })
            .expect("resolve scoped address");

        assert_eq!(resolved.to_string(), "[fe80::2222%7]:29717");
    }

    #[test]
    fn auto_peer_announce_socket_target_rejects_scope_on_ipv4() {
        let target = AutoPeerAnnounceSocketTarget {
            host: "127.0.0.1".to_string(),
            port: 29_717,
            scope_ifname: Some("eth0".to_string()),
        };

        let err = target.resolve_socket_addr(|_| Ok(7)).expect_err("IPv4 scope should be rejected");

        assert!(err.contains("IPv4 destination 127.0.0.1 cannot use scope interface eth0"));
    }

    #[tokio::test]
    async fn auto_initial_peer_announces_udp_socket_sender_transmits_payload() {
        let receiver = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind receiver");
        let receiver_addr = receiver.local_addr().expect("receiver addr");
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let token = [0x42; rns_transport::hash::HASH_SIZE];
        let plan = AutoDaemonStartupPlan {
            config: AutoInterfaceConfig::default(),
            platform: AutoInterfacePlatform::Other,
            device_filter: AutoInterfaceDeviceFilter::default(),
            candidates: Vec::new(),
            adopted_devices: Vec::new(),
            peering_packets: vec![AutoPeeringPacket {
                kind: AutoPeeringPacketKind::ReverseUnicast,
                ifname: "lo".to_string(),
                source_link_local_address: "127.0.0.1".to_string(),
                destination_address: receiver_addr.ip().to_string(),
                destination_port: receiver_addr.port(),
                token,
            }],
            startup_plan: empty_startup_plan(),
        };

        let count = plan
            .send_initial_peer_announces_with_udp_socket(&sender, |_| {
                panic!("IPv4 target should not need a scope id")
            })
            .await
            .expect("send datagram");

        let mut payload = [0u8; rns_transport::hash::HASH_SIZE];
        let (received, _) = receiver.recv_from(&mut payload).await.expect("receive datagram");
        assert_eq!(count, 1);
        assert_eq!(received, rns_transport::hash::HASH_SIZE);
        assert_eq!(payload, token);
    }

    #[tokio::test]
    async fn auto_bind_unicast_discovery_sockets_binds_loopback_listener() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "::1".to_string(),
            unicast_bind_address: "::1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d".to_string(),
            multicast_bind_address: "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d".to_string(),
            multicast_bind_port: 48_555,
        });

        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("loopback unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");

        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].kind, AutoDiscoverySocketKind::Unicast);
        assert_eq!(sockets[0].ifname, "lo");
        assert_eq!(sockets[0].multicast_group_addr, None);
        assert!(sockets[0].bind_addr.is_ipv6());
        assert_ne!(sockets[0].bind_addr.port(), 0);
    }

    #[tokio::test]
    async fn auto_bind_peer_data_socket_receives_typed_datagram() {
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
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let payload = b"auto-peer-data";

        sender.send_to(payload, sockets[0].bind_addr).await.expect("send peer data datagram");
        let datagram = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sockets[0].recv_peer_data_datagram(),
        )
        .await
        .expect("receive timeout")
        .expect("receive peer data datagram");

        assert_eq!(datagram.ifname, "lo");
        assert_eq!(datagram.bind_addr, sockets[0].bind_addr);
        assert_eq!(datagram.source_addr.ip(), sender.local_addr().expect("sender addr").ip());
        assert_eq!(datagram.payload, payload);
    }

    #[tokio::test]
    async fn auto_bound_discovery_socket_receives_typed_datagram() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            unicast_bind_address: "127.0.0.1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "239.255.0.1".to_string(),
            multicast_bind_address: "239.255.0.1".to_string(),
            multicast_bind_port: 0,
        });
        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let payload = b"auto-discovery-token";

        sender.send_to(payload, sockets[0].bind_addr).await.expect("send discovery datagram");
        let datagram = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sockets[0].recv_discovery_datagram(),
        )
        .await
        .expect("receive timeout")
        .expect("receive discovery datagram");

        assert_eq!(datagram.kind, AutoDiscoverySocketKind::Unicast);
        assert_eq!(datagram.ifname, "lo");
        assert_eq!(datagram.bind_addr, sockets[0].bind_addr);
        assert_eq!(datagram.multicast_group_addr, None);
        assert_eq!(datagram.source_addr.ip(), sender.local_addr().expect("sender addr").ip());
        assert_eq!(datagram.payload, payload);
    }

    #[tokio::test]
    async fn auto_discovery_listener_supervisor_stops_managed_listener() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            unicast_bind_address: "127.0.0.1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "239.255.0.1".to_string(),
            multicast_bind_address: "239.255.0.1".to_string(),
            multicast_bind_port: 0,
        });
        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");
        let bind_addr = sockets[0].bind_addr;
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut supervisor =
            AutoDiscoveryListenerSupervisor::new(plan.clone(), Arc::clone(&state), shutdown_rx);
        supervisor.spawn_sockets(sockets, &events_tx);
        assert_eq!(supervisor.receive_loop_count(), 1);

        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        let payload = rns_transport::iface::auto::peering_token(
            plan.config.group_id.as_bytes(),
            &source_address,
        );

        sender.send_to(&payload, bind_addr).await.expect("send valid discovery datagram");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("accepted event timeout")
            .expect("accepted event");
        assert!(matches!(
            accepted,
            AutoDiscoveryLoopEvent::Processed(AutoProcessedDiscoveryDatagram {
                event: AutoDiscoveryEvent::Peer(_),
                ..
            })
        ));

        supervisor.shutdown_all().await;
        sender
            .send_to(&payload, bind_addr)
            .await
            .expect("send after supervised shutdown");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), events_rx.recv())
                .await
                .is_err(),
            "stopped discovery listener should not emit events"
        );
    }

    #[tokio::test]
    async fn auto_discovery_listener_supervisor_tracks_replaced_listener_shutdown() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            unicast_bind_address: "127.0.0.1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "239.255.0.1".to_string(),
            multicast_bind_address: "239.255.0.1".to_string(),
            multicast_bind_port: 0,
        });
        let first_sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind first unicast discovery socket");
        let second_sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind second unicast discovery socket");
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(4);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut supervisor =
            AutoDiscoveryListenerSupervisor::new(plan.clone(), Arc::clone(&state), shutdown_rx);

        supervisor.spawn_sockets(first_sockets, &events_tx);
        assert_eq!(supervisor.receive_loop_count(), 1);
        assert_eq!(supervisor.pending_stop_count(), 0);

        supervisor.spawn_sockets(second_sockets, &events_tx);
        assert_eq!(supervisor.receive_loop_count(), 1);
        assert_eq!(supervisor.pending_stop_count(), 1);

        supervisor.shutdown_all().await;
        assert_eq!(supervisor.receive_loop_count(), 0);
        assert_eq!(supervisor.pending_stop_count(), 0);
    }

    #[tokio::test]
    async fn auto_discovery_receive_loop_authenticates_datagrams_and_reports_events() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            unicast_bind_address: "127.0.0.1".to_string(),
            unicast_bind_port: 0,
            multicast_group_address: "239.255.0.1".to_string(),
            multicast_bind_address: "239.255.0.1".to_string(),
            multicast_bind_port: 0,
        });
        let sockets = plan
            .bind_unicast_discovery_sockets(|_| panic!("IPv4 unicast bind is unscoped"))
            .await
            .expect("bind unicast discovery socket");
        let bind_addr = sockets[0].bind_addr;
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handles =
            plan.spawn_discovery_receive_loops(sockets, Arc::clone(&state), events_tx, shutdown_rx);
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        let payload = rns_transport::iface::auto::peering_token(
            plan.config.group_id.as_bytes(),
            &source_address,
        );

        sender.send_to(&payload, bind_addr).await.expect("send valid discovery datagram");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("accepted event timeout")
            .expect("accepted event");

        match accepted {
            AutoDiscoveryLoopEvent::Processed(processed) => {
                assert_eq!(processed.source_address, source_address);
                assert_eq!(
                    processed.event,
                    AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
                );
            }
            other => panic!("unexpected accepted event: {other:?}"),
        }
        assert!(state.lock().await.peer(&source_address).is_some());

        sender
            .send_to(&[0; rns_transport::hash::HASH_SIZE], bind_addr)
            .await
            .expect("send invalid discovery datagram");
        let rejected = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("rejected event timeout")
            .expect("rejected event");

        match rejected {
            AutoDiscoveryLoopEvent::Rejected {
                source_address: rejected_source, reason, ..
            } => {
                assert_eq!(rejected_source, source_address);
                assert_eq!(reason, AutoDiscoveryRejectReason::InvalidToken);
            }
            other => panic!("unexpected rejected event: {other:?}"),
        }

        shutdown_tx.send(true).expect("send shutdown");
        for handle in handles {
            tokio::time::timeout(std::time::Duration::from_secs(1), handle)
                .await
                .expect("receive loop shutdown timeout")
                .expect("receive loop task");
        }
    }

    #[tokio::test]
    async fn auto_peer_data_receive_loop_accepts_known_peer_and_suppresses_duplicate() {
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
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handles = plan.spawn_peer_data_receive_loops(
            sockets,
            Arc::clone(&state),
            Arc::clone(&dedupe),
            None,
            events_tx,
            shutdown_rx,
        );
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        state.lock().await.observe_discovery_packet(
            &source_address,
            "lo",
            core::time::Duration::ZERO,
        );

        sender.send_to(b"packet", bind_addr).await.expect("send peer data datagram");
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("accepted event timeout")
            .expect("accepted event");

        match accepted {
            AutoPeerDataLoopEvent::Processed(processed) => {
                assert_eq!(processed.peer_address, source_address);
                assert_eq!(processed.datagram.payload, b"packet");
                assert!(matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }));
            }
            other => panic!("unexpected accepted peer-data event: {other:?}"),
        }

        sender.send_to(b"packet", bind_addr).await.expect("send duplicate peer data datagram");
        let duplicate = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("duplicate event timeout")
            .expect("duplicate event");

        match duplicate {
            AutoPeerDataLoopEvent::Processed(processed) => {
                assert_eq!(processed.peer_address, source_address);
                assert_eq!(processed.decision, AutoPeerInboundDecision::Duplicate);
            }
            other => panic!("unexpected duplicate peer-data event: {other:?}"),
        }

        shutdown_tx.send(true).expect("send shutdown");
        for handle in handles {
            tokio::time::timeout(std::time::Duration::from_secs(1), handle)
                .await
                .expect("peer data loop shutdown timeout")
                .expect("peer data loop task");
        }
    }

    #[tokio::test]
    async fn auto_peer_data_listener_supervisor_tracks_replaced_listener_shutdown() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        let first_socket = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind first peer data socket")
            .remove(0);
        let second_socket = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind second peer data socket")
            .remove(0);
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(4);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut supervisor = AutoPeerDataListenerSupervisor::new(
            plan.clone(),
            Arc::clone(&state),
            dedupe,
            None,
            shutdown_rx,
        );

        supervisor.spawn_bound_socket(first_socket, &events_tx);
        assert_eq!(supervisor.len(), 1);
        assert_eq!(supervisor.pending_stop_count(), 0);

        supervisor.spawn_bound_socket(second_socket, &events_tx);
        assert_eq!(supervisor.len(), 1);
        assert_eq!(supervisor.pending_stop_count(), 1);

        supervisor.shutdown_all().await;
        assert_eq!(supervisor.len(), 0);
        assert_eq!(supervisor.pending_stop_count(), 0);
    }
