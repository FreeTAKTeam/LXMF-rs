    fn loopback_software_auto_plan(
        initial_peering_wait: core::time::Duration,
    ) -> AutoDaemonStartupPlan {
        let config = AutoInterfaceConfig {
            group_id: "software-auto".to_string(),
            discovery_scope: AutoDiscoveryScope::Global,
            multicast_address_type: MulticastAddressType::Permanent,
            discovery_port: 48_555,
            data_port: 0,
        };
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.2".to_string(),
        };
        AutoDaemonStartupPlan {
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
                initial_peering_wait,
            },
        }
    }

    #[tokio::test]
    async fn auto_software_discovery_regression_covers_peer_admission_dedupe_tx_and_status() {
        let mut plan = loopback_software_auto_plan(core::time::Duration::from_millis(50));
        let peer_socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind software peer socket");
        let peer_addr = peer_socket.local_addr().expect("software peer addr");
        let peer_address = peer_addr.ip().to_string();
        let bind_addr = "127.0.0.1:0".parse().expect("software bind addr");
        let discovery_payload = rns_transport::iface::auto::peering_token(
            plan.config.group_id.as_bytes(),
            &peer_address,
        )
        .to_vec();
        let discovery_datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: "lo".to_string(),
            bind_addr,
            multicast_group_addr: None,
            source_addr: peer_addr,
            payload: discovery_payload,
        };
        let invalid_discovery_datagram = AutoDiscoveryDatagram {
            payload: vec![0; rns_transport::hash::HASH_SIZE],
            ..discovery_datagram.clone()
        };
        let mut state = plan.discovery_state();
        let before_final_init =
            plan.startup_plan.initial_peering_wait - core::time::Duration::from_millis(1);

        assert_eq!(
            plan.process_discovery_datagram(
                &mut state,
                discovery_datagram.clone(),
                before_final_init,
            ),
            Ok(None)
        );
        assert_eq!(
            plan.process_discovery_datagram(
                &mut state,
                invalid_discovery_datagram,
                before_final_init,
            ),
            Ok(None)
        );
        assert!(state.peer(&peer_address).is_none());

        let admitted = plan
            .process_discovery_datagram(
                &mut state,
                discovery_datagram,
                plan.startup_plan.initial_peering_wait,
            )
            .expect("authenticated discovery datagram")
            .expect("final init should admit discovery");
        assert_eq!(admitted.source_address, peer_address);
        assert_eq!(
            admitted.event,
            AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
        );
        assert!(state.peer(&peer_address).is_some());

        let startup_runtime = plan.runtime_json();
        assert_eq!(
            startup_runtime.get("auto_runtime_status").and_then(JsonValue::as_str),
            Some("complete")
        );
        assert_eq!(
            startup_runtime
                .get("planned_discovery_receive_loop_count")
                .and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            startup_runtime
                .get("planned_data_receive_loop_count")
                .and_then(JsonValue::as_u64),
            Some(1)
        );

        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let rx_recv = iface_manager.lock().await.receiver();
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let host_iface = channel.address;
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let tx_handle =
            plan.spawn_peer_data_transport_tx_loop(bridge.clone(), tx_channel, shutdown_rx);
        let route_socket = Arc::new(
            tokio::net::UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("bind AutoInterface route socket"),
        );
        let route_bind_addr = route_socket.local_addr().expect("route bind addr");
        let inbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x33; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"software-ingress"),
            ..Default::default()
        };
        let inbound_payload = inbound_packet.to_bytes().expect("serialize inbound packet");
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );
        assert_eq!(
            plan.process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                AutoPeerDataDatagram {
                    ifname: "lo".to_string(),
                    bind_addr: route_bind_addr,
                    source_addr: peer_addr,
                    payload: inbound_payload.clone(),
                },
                before_final_init,
            ),
            None
        );
        assert!(dedupe.is_empty());

        plan.startup_plan.initial_peering_wait = core::time::Duration::ZERO;
        let runtime_status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
        let accepted = plan
            .process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                AutoPeerDataDatagram {
                    ifname: "lo".to_string(),
                    bind_addr: route_bind_addr,
                    source_addr: peer_addr,
                    payload: inbound_payload.clone(),
                },
                core::time::Duration::from_millis(51),
            )
            .expect("peer data after final init");
        assert_eq!(accepted.peer_address, peer_address);
        assert!(matches!(accepted.decision, AutoPeerInboundDecision::Accepted { .. }));
        assert_eq!(dedupe.len(), 1);

        let accepted_forward = bridge.forward_peer_data(&accepted, Arc::clone(&route_socket)).await;
        assert_eq!(accepted_forward, AutoPeerDataForwardResult::Delivered);
        runtime_status.record_peer_data(&accepted, Some(accepted_forward));
        let rx_message =
            tokio::time::timeout(std::time::Duration::from_secs(1), rx_recv.lock().await.recv())
                .await
                .expect("transport rx message timeout")
                .expect("transport rx message");
        let virtual_iface = rx_message.address;
        assert_eq!(rx_message.packet, inbound_packet);
        assert_eq!(rx_message.source, IfaceSource::Udp(peer_addr));
        assert_eq!(
            iface_manager.lock().await.role(&virtual_iface),
            Some(IfaceRole::VirtualUnicast)
        );

        let malformed = plan
            .process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                AutoPeerDataDatagram {
                    ifname: "lo".to_string(),
                    bind_addr: route_bind_addr,
                    source_addr: peer_addr,
                    payload: b"not-a-reticulum-packet".to_vec(),
                },
                core::time::Duration::from_millis(52),
            )
            .expect("malformed known-peer data after final init");
        assert!(matches!(malformed.decision, AutoPeerInboundDecision::Accepted { .. }));
        let malformed_forward =
            bridge.forward_peer_data(&malformed, Arc::clone(&route_socket)).await;
        assert_eq!(malformed_forward, AutoPeerDataForwardResult::DecodeFailed);
        runtime_status.record_peer_data(&malformed, Some(malformed_forward));
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                rx_recv.lock().await.recv(),
            )
            .await
            .is_err(),
            "malformed peer data should not ingress through the transport bridge"
        );

        let duplicate = plan
            .process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                AutoPeerDataDatagram {
                    ifname: "lo".to_string(),
                    bind_addr: route_bind_addr,
                    source_addr: peer_addr,
                    payload: inbound_payload,
                },
                core::time::Duration::from_millis(53),
            )
            .expect("duplicate peer data after final init");
        assert_eq!(duplicate.decision, AutoPeerInboundDecision::Duplicate);
        assert_eq!(dedupe.len(), 2);
        let duplicate_forward =
            bridge.forward_peer_data(&duplicate, Arc::clone(&route_socket)).await;
        assert_eq!(duplicate_forward, AutoPeerDataForwardResult::NotForwarded);
        runtime_status.record_peer_data(&duplicate, Some(duplicate_forward));
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                rx_recv.lock().await.recv(),
            )
            .await
            .is_err(),
            "duplicate peer data should not ingress through the transport bridge"
        );

        let outbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x44; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"software-egress"),
            ..Default::default()
        };
        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(virtual_iface),
                packet: outbound_packet.clone(),
            })
            .await;
        let mut outbound_payload = [0u8; 512];
        let (received, _) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            peer_socket.recv_from(&mut outbound_payload),
        )
        .await
        .expect("direct Tx receive timeout")
        .expect("direct Tx receive");
        let decoded = Packet::deserialize(&mut InputBuffer::new(&outbound_payload[..received]))
            .expect("decode direct Tx packet");
        assert_eq!(decoded, outbound_packet);

        let (closed_rx_channel, closed_rx_recv) = tokio::sync::mpsc::channel(1);
        drop(closed_rx_recv);
        let closed_bridge = AutoInterfaceTransportBridge {
            host_iface,
            iface_manager: Arc::clone(&iface_manager),
            rx_channel: closed_rx_channel,
            peer_ifaces: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            outbound_routes: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        };
        let closed_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x55; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"rx-closed"),
            ..Default::default()
        };
        let closed_payload = closed_packet.to_bytes().expect("serialize closed-channel packet");
        let closed_channel = plan
            .process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                AutoPeerDataDatagram {
                    ifname: "lo".to_string(),
                    bind_addr: route_bind_addr,
                    source_addr: peer_addr,
                    payload: closed_payload,
                },
                core::time::Duration::from_millis(54),
            )
            .expect("closed-channel known-peer data after final init");
        let closed_forward =
            closed_bridge.forward_peer_data(&closed_channel, Arc::clone(&route_socket)).await;
        assert_eq!(closed_forward, AutoPeerDataForwardResult::RxChannelClosed);
        runtime_status.record_peer_data(&closed_channel, Some(closed_forward));

        let peer_job = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(5_252), |_| Ok(()))
            .expect("peer job summary for runtime status");
        runtime_status.record_peer_job_summary(&peer_job);
        let carrier_runtime = runtime_status.to_json();
        assert_eq!(
            carrier_runtime.get("final_init_done").and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            carrier_runtime.get("adopted_device_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime
                .get("last_peer_job")
                .and_then(|summary| summary.get("peer_count_after"))
                .and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime
                .get("last_peer_job")
                .and_then(|summary| summary.get("reverse_peer_announce_count"))
                .and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime.get("peer_data_admitted_count").and_then(JsonValue::as_u64),
            Some(3)
        );
        assert_eq!(
            carrier_runtime.get("peer_data_duplicate_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime.get("peer_data_delivered_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime
                .get("peer_data_decode_failed_count")
                .and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime.get("peer_data_rx_closed_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            carrier_runtime
                .get("last_peer_data")
                .and_then(|summary| summary.get("decision"))
                .and_then(JsonValue::as_str),
            Some("accepted")
        );
        assert_eq!(
            carrier_runtime
                .get("last_peer_data")
                .and_then(|summary| summary.get("forwarding"))
                .and_then(JsonValue::as_str),
            Some("rx_channel_closed")
        );

        shutdown_tx.send(true).expect("send shutdown");
        tokio::time::timeout(std::time::Duration::from_secs(1), tx_handle)
            .await
            .expect("tx loop shutdown timeout")
            .expect("tx loop task");
    }

    #[tokio::test]
    async fn auto_peer_data_transport_bridge_registers_virtual_iface_and_routes_direct_tx() {
        let mut plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        plan.startup_plan.initial_peering_wait = core::time::Duration::ZERO;
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
                packet: outbound_packet.clone(),
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

    #[tokio::test]
    async fn auto_peer_data_listener_removal_prunes_direct_tx_route() {
        let mut plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        plan.startup_plan.initial_peering_wait = core::time::Duration::ZERO;
        let sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        let bind_addr = sockets[0].bind_addr;
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let rx_recv = iface_manager.lock().await.receiver();
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let tx_handle =
            plan.spawn_peer_data_transport_tx_loop(bridge.clone(), tx_channel, shutdown_rx.clone());
        let mut data_supervisor = AutoPeerDataListenerSupervisor::new(
            plan,
            Arc::clone(&state),
            dedupe,
            Some(bridge),
            shutdown_rx,
        );
        data_supervisor.spawn_sockets(sockets, &events_tx);
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
                packet: outbound_packet.clone(),
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

        assert!(data_supervisor.remove_listener("lo").await);

        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(rx_message.address),
                packet: outbound_packet,
            })
            .await;
        let stale_result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            sender.recv_from(&mut outbound_payload),
        )
        .await;
        assert!(stale_result.is_err(), "stale peer-data route still emitted direct Tx");

        shutdown_tx.send(true).expect("send shutdown");
        data_supervisor.shutdown_all().await;
        tokio::time::timeout(std::time::Duration::from_secs(1), tx_handle)
            .await
            .expect("tx loop shutdown timeout")
            .expect("tx loop task");
    }

    #[tokio::test]
    async fn auto_peer_data_listener_restart_prunes_and_refreshes_direct_tx_route() {
        let mut plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "lo".to_string(),
            link_local_address: "127.0.0.1".to_string(),
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
        });
        plan.startup_plan.initial_peering_wait = core::time::Duration::ZERO;
        let sockets = plan
            .bind_data_sockets(|_| panic!("IPv4 data bind is unscoped"))
            .await
            .expect("bind peer data socket");
        let old_bind_addr = sockets[0].bind_addr;
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let rx_recv = iface_manager.lock().await.receiver();
        let channel = iface_manager.lock().await.new_channel_with_role(8, IfaceRole::Multicast);
        let runtime =
            AutoInterfaceTransportRuntime::from_channel(channel, Arc::clone(&iface_manager));
        let (bridge, tx_channel) = runtime.split();
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let tx_handle =
            plan.spawn_peer_data_transport_tx_loop(bridge.clone(), tx_channel, shutdown_rx.clone());
        let mut data_supervisor = AutoPeerDataListenerSupervisor::new(
            plan,
            Arc::clone(&state),
            dedupe,
            Some(bridge),
            shutdown_rx,
        );
        data_supervisor.spawn_sockets(sockets, &events_tx);
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        let source_address = sender.local_addr().expect("sender addr").ip().to_string();
        state.lock().await.observe_discovery_packet(
            &source_address,
            "lo",
            core::time::Duration::ZERO,
        );
        let inbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x66; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"restart-before"),
            ..Default::default()
        };
        let inbound_payload = inbound_packet.to_bytes().expect("serialize inbound packet");

        sender.send_to(&inbound_payload, old_bind_addr).await.expect("send peer data datagram");
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
        let virtual_iface = rx_message.address;
        assert_eq!(rx_message.packet, inbound_packet);

        let outbound_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x77; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"restart-route"),
            ..Default::default()
        };
        let update = AutoLinkLocalAddressUpdate {
            ifname: "lo".to_string(),
            old_link_local_address: "127.0.0.1".to_string(),
            new_link_local_address: "127.0.0.2".to_string(),
            listener_binding: AutoDataListenerBinding {
                ifname: "lo".to_string(),
                link_local_address: "127.0.0.2".to_string(),
                bind_address: "127.0.0.1".to_string(),
                bind_port: 0,
            },
        };
        let new_bind_addr = data_supervisor
            .restart_link_local_listener(
                &update,
                None,
                &events_tx,
                |_| panic!("IPv4 data bind is unscoped"),
            )
            .await
            .expect("restart link-local data listener");
        assert_ne!(new_bind_addr, old_bind_addr);

        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(virtual_iface),
                packet: outbound_packet.clone(),
            })
            .await;
        let mut outbound_payload = [0u8; 512];
        let stale_result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            sender.recv_from(&mut outbound_payload),
        )
        .await;
        assert!(stale_result.is_err(), "stale restarted peer-data route still emitted direct Tx");

        let refreshed_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x88; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"restart-after"),
            ..Default::default()
        };
        let refreshed_payload = refreshed_packet.to_bytes().expect("serialize refreshed packet");
        sender
            .send_to(&refreshed_payload, new_bind_addr)
            .await
            .expect("send refreshed peer data datagram");
        let refreshed = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("refreshed event timeout")
            .expect("refreshed event");
        assert!(matches!(
            refreshed,
            AutoPeerDataLoopEvent::Processed(AutoProcessedPeerDataDatagram {
                decision: AutoPeerInboundDecision::Accepted { .. },
                ..
            })
        ));
        let refreshed_rx =
            tokio::time::timeout(std::time::Duration::from_secs(1), rx_recv.lock().await.recv())
                .await
                .expect("refreshed rx message timeout")
                .expect("refreshed rx message");
        assert_eq!(refreshed_rx.address, virtual_iface);
        assert_eq!(refreshed_rx.packet, refreshed_packet);

        iface_manager
            .lock()
            .await
            .send(TxMessage {
                tx_type: TxMessageType::Direct(virtual_iface),
                packet: outbound_packet.clone(),
            })
            .await;
        let (received, _) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sender.recv_from(&mut outbound_payload),
        )
        .await
        .expect("refreshed outbound receive timeout")
        .expect("refreshed outbound receive");
        let decoded = Packet::deserialize(&mut InputBuffer::new(&outbound_payload[..received]))
            .expect("decode refreshed outbound packet");
        assert_eq!(decoded, outbound_packet);

        shutdown_tx.send(true).expect("send shutdown");
        data_supervisor.shutdown_all().await;
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
            .expect("authenticated local echo")
            .expect("final init should allow discovery processing");

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
            .expect("authenticated remote peer")
            .expect("final init should allow discovery processing");

        assert_eq!(processed.source_address, "fe80::2222");
        assert_eq!(
            processed.event,
            AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
        );
        assert!(state.peer("fe80::2222").is_some());
    }

    #[test]
    fn auto_process_discovery_datagram_ignores_before_final_init() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let valid_datagram = AutoDiscoveryDatagram {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: "eth0".to_string(),
            bind_addr: "[fe80::1234]:48556".parse().expect("bind addr"),
            multicast_group_addr: None,
            source_addr: "[fe80::2222]:48556".parse().expect("source addr"),
            payload: rns_transport::iface::auto::peering_token(b"field-net", "fe80::2222").to_vec(),
        };
        let invalid_datagram =
            AutoDiscoveryDatagram { payload: vec![0; rns_transport::hash::HASH_SIZE], ..valid_datagram.clone() };
        let before_final_init =
            plan.startup_plan.initial_peering_wait - core::time::Duration::from_millis(1);

        assert_eq!(
            plan.process_discovery_datagram(&mut state, valid_datagram.clone(), before_final_init),
            Ok(None)
        );
        assert_eq!(
            plan.process_discovery_datagram(&mut state, invalid_datagram, before_final_init),
            Ok(None)
        );
        assert!(state.peer("fe80::2222").is_none());

        let processed = plan
            .process_discovery_datagram(
                &mut state,
                valid_datagram,
                plan.startup_plan.initial_peering_wait,
            )
            .expect("authenticated remote peer")
            .expect("final init should allow discovery processing");
        assert_eq!(
            processed.event,
            AutoDiscoveryEvent::Peer(rns_transport::iface::auto::AutoPeerEvent::Added)
        );
        assert!(state.peer("fe80::2222").is_some());
    }

    #[test]
    fn auto_process_peer_data_datagram_ignores_before_final_init() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        state.observe_discovery_packet(
            "fe80::2222",
            "eth0",
            plan.startup_plan.initial_peering_wait,
        );
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );
        let datagram = AutoPeerDataDatagram {
            ifname: "eth0".to_string(),
            bind_addr: "[fe80::1234]:48557".parse().expect("bind addr"),
            source_addr: "[fe80::2222]:48557".parse().expect("source addr"),
            payload: b"packet".to_vec(),
        };
        let before_final_init =
            plan.startup_plan.initial_peering_wait - core::time::Duration::from_millis(1);

        assert_eq!(
            plan.process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                datagram.clone(),
                before_final_init,
            ),
            None
        );

        let processed = plan
            .process_peer_data_datagram(
                &mut state,
                &mut dedupe,
                datagram,
                plan.startup_plan.initial_peering_wait,
            )
            .expect("final init should allow peer data processing");
        assert_eq!(processed.peer_address, "fe80::2222");
        assert!(matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }));
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
