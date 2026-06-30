    use super::*;

    fn auto_iface() -> InterfaceConfig {
        InterfaceConfig {
            kind: "auto".to_string(),
            group_id: Some("field-net".to_string()),
            discovery_scope: Some("global".to_string()),
            multicast_address_type: Some("permanent".to_string()),
            discovery_port: Some(48_555),
            data_port: Some(49_555),
            devices: Some(vec!["eth0".to_string()]),
            ignored_devices: Some(vec!["tun0".to_string()]),
            ..InterfaceConfig::default()
        }
    }

    fn default_link_auto_iface() -> InterfaceConfig {
        InterfaceConfig {
            kind: "auto".to_string(),
            group_id: Some("reticulum".to_string()),
            discovery_scope: Some("link".to_string()),
            multicast_address_type: Some("temporary".to_string()),
            discovery_port: Some(29_716),
            data_port: Some(42_671),
            devices: Some(vec!["eth0".to_string()]),
            ..InterfaceConfig::default()
        }
    }

    fn empty_startup_plan() -> AutoStartupPlan {
        AutoStartupPlan {
            discovery_listeners: Vec::new(),
            data_listeners: Vec::new(),
            peer_job_interval: core::time::Duration::ZERO,
            initial_peering_wait: core::time::Duration::ZERO,
        }
    }

    fn plan_with_discovery_listener(
        listener: AutoDiscoveryListenerBinding,
    ) -> AutoDaemonStartupPlan {
        AutoDaemonStartupPlan {
            config: AutoInterfaceConfig::default(),
            platform: AutoInterfacePlatform::Other,
            device_filter: AutoInterfaceDeviceFilter::default(),
            candidates: Vec::new(),
            adopted_devices: Vec::new(),
            peering_packets: Vec::new(),
            startup_plan: AutoStartupPlan {
                discovery_listeners: vec![listener],
                data_listeners: Vec::new(),
                peer_job_interval: core::time::Duration::ZERO,
                initial_peering_wait: core::time::Duration::ZERO,
            },
        }
    }

    fn plan_with_data_listener(listener: AutoDataListenerBinding) -> AutoDaemonStartupPlan {
        AutoDaemonStartupPlan {
            config: AutoInterfaceConfig::default(),
            platform: AutoInterfacePlatform::Other,
            device_filter: AutoInterfaceDeviceFilter::default(),
            candidates: Vec::new(),
            adopted_devices: Vec::new(),
            peering_packets: Vec::new(),
            startup_plan: AutoStartupPlan {
                discovery_listeners: Vec::new(),
                data_listeners: vec![listener],
                peer_job_interval: core::time::Duration::ZERO,
                initial_peering_wait: core::time::Duration::ZERO,
            },
        }
    }

    #[test]
    fn auto_interface_index_resolver_uses_indexed_interfaces_only() {
        let resolver = AutoInterfaceIndexResolver::from_index_entries([
            ("eth0".to_string(), Some(7)),
            ("lo".to_string(), None),
            ("wlan0".to_string(), Some(11)),
        ]);

        assert_eq!(resolver.resolve("eth0"), Ok(7));
        assert_eq!(resolver.resolve("wlan0"), Ok(11));
        assert_eq!(resolver.resolve("lo"), Err("interface index for lo was not found".to_string()));
        assert_eq!(
            resolver.resolve("missing0"),
            Err("interface index for missing0 was not found".to_string())
        );
    }

    #[test]
    fn auto_interface_index_resolver_drives_scoped_socket_resolution() {
        let resolver =
            AutoInterfaceIndexResolver::from_index_entries([("eth0".to_string(), Some(7))]);
        let target = AutoPeerAnnounceSocketTarget {
            host: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            port: 29_716,
            scope_ifname: Some("eth0".to_string()),
        };

        let resolved = target.resolve_socket_addr(|ifname| resolver.resolve(ifname)).unwrap();

        assert_eq!(resolved.to_string(), "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%7]:29716");
    }

    #[test]
    fn auto_startup_plan_adopts_configured_link_local_candidates() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![
                AutoInterfaceDeviceCandidate {
                    ifname: "eth0".to_string(),
                    ipv6_addresses: vec!["fe80::1234".to_string()],
                },
                AutoInterfaceDeviceCandidate {
                    ifname: "wlan0".to_string(),
                    ipv6_addresses: vec!["fe80::5678".to_string()],
                },
                AutoInterfaceDeviceCandidate {
                    ifname: "tun0".to_string(),
                    ipv6_addresses: vec!["fe80::9999".to_string()],
                },
            ],
        )
        .expect("startup plan");

        assert_eq!(
            plan.adopted_devices,
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1234".to_string(),
            }]
        );
        assert_eq!(plan.startup_plan.discovery_listeners.len(), 1);
        assert_eq!(plan.startup_plan.data_listeners.len(), 1);
        assert_eq!(plan.startup_plan.data_listeners[0].bind_port, 49_555);
        assert_eq!(plan.peering_packets.len(), 1);
        assert_eq!(plan.peering_packets[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(plan.peering_packets[0].ifname, "eth0");
        assert_eq!(plan.peering_packets[0].destination_port, 48_555);
        assert_eq!(plan.peering_packets[0].payload(), &plan.peering_packets[0].token);
        assert_eq!(plan.initial_peer_announce_datagrams().len(), 1);
        assert_eq!(
            plan.initial_peer_announce_datagrams()[0].payload,
            plan.peering_packets[0].token.to_vec()
        );
    }

    #[test]
    fn auto_runtime_json_exposes_complete_socket_runtime_plan() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let runtime = plan.runtime_json();

        assert_eq!(
            runtime.get("auto_runtime_status").and_then(JsonValue::as_str),
            Some("complete")
        );
        assert_eq!(
            runtime
                .get("startup_plan")
                .and_then(|value| value.get("data_listeners"))
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            runtime
                .get("initial_peer_announces")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("kind"))
                .and_then(JsonValue::as_str),
            Some("multicast")
        );
        assert!(runtime
            .get("initial_peer_announces")
            .and_then(JsonValue::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("payload_hex"))
            .and_then(JsonValue::as_str)
            .is_some_and(|payload| payload.len() == rns_transport::hash::HASH_SIZE * 2));
        assert_eq!(
            runtime
                .get("initial_peer_announces")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("destination_socket_target"))
                .and_then(JsonValue::as_str),
            Some("[ff0e:0:77b9:4bfd:9488:364b:4bbe:119d]:48555")
        );
        assert_eq!(
            runtime.get("planned_initial_peer_announce_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_repeat_peer_announce_scheduler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_peer_job_scheduler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_adopted_interface_reconciler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime
                .get("planned_discovery_socket_binds")
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            runtime.get("planned_discovery_receive_loop_count").and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            runtime.get("planned_data_socket_binds").and_then(JsonValue::as_array).map(Vec::len),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_data_receive_loop_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("native_scope_id_source").and_then(JsonValue::as_str),
            Some("if-addrs interface index")
        );
        assert_eq!(
            runtime
                .get("initial_peer_announces")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("destination_scope_ifname"))
                .and_then(JsonValue::as_str),
            None
        );
        assert_eq!(
            runtime
                .get("carrier_runtime")
                .and_then(|value| value.get("carrier_changed"))
                .and_then(JsonValue::as_bool),
            Some(false)
        );
        assert_eq!(
            runtime
                .get("carrier_runtime")
                .and_then(|value| value.get("carrier_event_count"))
                .and_then(JsonValue::as_u64),
            Some(0)
        );
        assert_eq!(
            runtime
                .get("carrier_runtime")
                .and_then(|value| value.get("link_local_update")),
            Some(&JsonValue::Null)
        );
    }

    #[test]
    fn auto_runtime_json_keeps_zero_initial_runtime_schedulers_planned() {
        let plan =
            build_startup_plan_from_candidates(&auto_iface(), Vec::new()).expect("startup plan");
        let runtime = plan.runtime_json();

        assert_eq!(
            runtime.get("planned_initial_peer_announce_count").and_then(JsonValue::as_u64),
            Some(0)
        );
        assert_eq!(
            runtime.get("planned_discovery_receive_loop_count").and_then(JsonValue::as_u64),
            Some(0)
        );
        assert_eq!(
            runtime.get("planned_data_receive_loop_count").and_then(JsonValue::as_u64),
            Some(0)
        );
        assert_eq!(
            runtime.get("planned_repeat_peer_announce_scheduler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_peer_job_scheduler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime.get("planned_adopted_interface_reconciler_count").and_then(JsonValue::as_u64),
            Some(1)
        );
    }

    #[tokio::test]
    async fn auto_zero_initial_runtime_starts_reconciliation_schedulers() {
        let plan =
            build_startup_plan_from_candidates(&auto_iface(), Vec::new()).expect("startup plan");

        let summary = plan
            .spawn_discovery_runtime_with_native_scope_ids()
            .await
            .expect("start zero-initial runtime");

        assert_eq!(summary.bound_socket_count, 0);
        assert_eq!(summary.receive_loop_count, 0);
        assert_eq!(summary.initial_peer_announce_count, 0);
        assert_eq!(summary.data_socket_count, 0);
        assert_eq!(summary.data_receive_loop_count, 0);
        assert_eq!(summary.repeat_peer_announce_scheduler_count, 1);
        assert_eq!(summary.peer_job_scheduler_count, 1);
        assert_eq!(summary.adopted_interface_reconciler_count, 1);
    }

    #[test]
    fn auto_carrier_runtime_json_exposes_events_and_link_local_restart() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut runtime_state =
            AutoRuntimeState::from_startup_plan(&plan.startup_plan, core::time::Duration::ZERO);
        let carrier_events =
            vec![AutoMulticastCarrierEvent::CarrierLost { ifname: "eth0".to_string() }];
        let mut discovery_state = plan.discovery_state();
        let link_local_update = discovery_state
            .update_adopted_link_local_address(&plan.config, "eth0", "fe80::5678%eth0")
            .expect("link-local replacement");

        assert!(runtime_state.record_carrier_events(&carrier_events));
        assert!(runtime_state.record_link_local_update(Some(&link_local_update)));

        let runtime =
            auto_carrier_runtime_json(&runtime_state, &carrier_events, Some(&link_local_update));

        assert_eq!(
            runtime.get("carrier_changed").and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            runtime.get("carrier_event_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime
                .get("carrier_events")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("event"))
                .and_then(JsonValue::as_str),
            Some("carrier_lost")
        );
        assert_eq!(
            runtime
                .get("link_local_update")
                .and_then(|value| value.get("old_link_local_address"))
                .and_then(JsonValue::as_str),
            Some("fe80::1234")
        );
        assert_eq!(
            runtime
                .get("link_local_update")
                .and_then(|value| value.get("restart_data_listener"))
                .and_then(|value| value.get("bind_address"))
                .and_then(JsonValue::as_str),
            Some("fe80::5678%eth0")
        );
    }

    #[test]
    fn auto_multicast_announces_use_reconciled_link_local_address() {
        let plan = build_startup_plan_from_candidates(
            &default_link_auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let update = state
            .plan_adopted_link_local_address_update(&plan.config, "eth0", "fe80::5678%eth0")
            .expect("planned link-local replacement");
        state.apply_adopted_link_local_address_update(&update);

        let datagrams = plan.due_multicast_peer_announce_datagrams(
            &mut state,
            core::time::Duration::from_secs(2),
        );

        assert_eq!(datagrams.len(), 1);
        assert_eq!(datagrams[0].source_link_local_address, "fe80::5678");
    }

    #[test]
    fn auto_runtime_status_tracks_adopted_interface_churn() {
        let plan = build_startup_plan_from_candidates(&auto_iface(), Vec::new())
            .expect("zero-initial startup plan");
        let status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1234".to_string(),
        };
        let added = AutoAdoptedInterfaceChange::Added {
            discovery_listener: plan.config.discovery_listener_binding(&adopted, plan.platform),
            data_listener: plan.config.data_listener_binding(&adopted),
            adopted: adopted.clone(),
        };

        status.record_adopted_interface_change(&added);
        let runtime = status.to_json();
        assert_eq!(
            runtime.get("adopted_device_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(runtime.get("adopted_add_count").and_then(JsonValue::as_u64), Some(1));
        assert_eq!(
            runtime
                .get("last_adopted_change")
                .and_then(|value| value.get("event"))
                .and_then(JsonValue::as_str),
            Some("added")
        );

        let update = AutoLinkLocalAddressUpdate {
            ifname: "eth0".to_string(),
            old_link_local_address: "fe80::1234".to_string(),
            new_link_local_address: "fe80::5678".to_string(),
            listener_binding: plan.config.data_listener_binding(&AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::5678".to_string(),
            }),
        };
        assert!(status.record_link_local_update(Some(&update)));
        let runtime = status.to_json();
        assert_eq!(
            runtime.get("link_local_replacement_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            runtime
                .get("adopted_devices")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("link_local_address"))
                .and_then(JsonValue::as_str),
            Some("fe80::5678")
        );

        let removed = AutoAdoptedInterfaceChange::Removed {
            discovery_listener: plan.config.discovery_listener_binding(&adopted, plan.platform),
            data_listener: plan.config.data_listener_binding(&adopted),
            removed_peers: Vec::new(),
            adopted,
        };
        status.record_adopted_interface_change(&removed);
        let runtime = status.to_json();
        assert_eq!(
            runtime.get("adopted_device_count").and_then(JsonValue::as_u64),
            Some(0)
        );
        assert_eq!(runtime.get("adopted_remove_count").and_then(JsonValue::as_u64), Some(1));
        assert_eq!(
            runtime
                .get("last_adopted_change")
                .and_then(|value| value.get("event"))
                .and_then(JsonValue::as_str),
            Some("removed")
        );
    }

    #[tokio::test]
    async fn auto_peer_data_listener_supervisor_restarts_link_local_listener() {
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
            .expect("bind initial peer data socket");
        let old_bind_addr = sockets[0].bind_addr;
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(8);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut supervisor = AutoPeerDataListenerSupervisor::new(
            plan.clone(),
            Arc::clone(&state),
            dedupe,
            None,
            shutdown_rx,
        );
        supervisor.spawn_sockets(sockets, &events_tx);

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
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"restart"),
            ..Default::default()
        };
        let inbound_payload = inbound_packet.to_bytes().expect("serialize inbound packet");

        sender
            .send_to(&inbound_payload, old_bind_addr)
            .await
            .expect("send initial peer data datagram");
        let initial = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("initial event timeout")
            .expect("initial event");
        assert!(matches!(
            initial,
            AutoPeerDataLoopEvent::Processed(AutoProcessedPeerDataDatagram {
                decision: AutoPeerInboundDecision::Accepted { .. },
                ..
            })
        ));

        let runtime_status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
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
        let new_bind_addr = supervisor
            .restart_link_local_listener(
                &update,
                Some(&runtime_status),
                &events_tx,
                |_| panic!("IPv4 data bind is unscoped"),
            )
            .await
            .expect("restart link-local data listener");
        assert_ne!(new_bind_addr, old_bind_addr);

        let restarted_packet = Packet {
            destination: AddressHash::new_from_slice(
                &[0x77; rns_transport::hash::ADDRESS_HASH_SIZE],
            ),
            data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"restarted"),
            ..Default::default()
        };
        let restarted_payload = restarted_packet.to_bytes().expect("serialize restarted packet");
        sender
            .send_to(&restarted_payload, new_bind_addr)
            .await
            .expect("send restarted peer data datagram");
        let restarted = tokio::time::timeout(std::time::Duration::from_secs(1), events_rx.recv())
            .await
            .expect("restarted event timeout")
            .expect("restarted event");
        match restarted {
            AutoPeerDataLoopEvent::Processed(processed) => {
                assert_eq!(processed.datagram.bind_addr, new_bind_addr);
                assert!(matches!(processed.decision, AutoPeerInboundDecision::Accepted { .. }));
            }
            event => panic!("unexpected restarted event: {event:?}"),
        }

        sender
            .send_to(&inbound_payload, old_bind_addr)
            .await
            .expect("send to old peer data datagram");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), events_rx.recv())
                .await
                .is_err(),
            "old peer-data listener should not emit events after restart"
        );

        let runtime = runtime_status.to_json();
        assert_eq!(
            runtime.get("carrier_changed").and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            runtime
                .get("link_local_update")
                .and_then(|value| value.get("restart_data_listener"))
                .and_then(|value| value.get("link_local_address"))
                .and_then(JsonValue::as_str),
            Some("127.0.0.2")
        );

        supervisor.shutdown_all().await;
    }

    #[tokio::test]
    async fn auto_link_local_reconciler_failed_restart_does_not_commit_state() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1111".to_string()],
            }],
        )
        .expect("startup plan");
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (events_tx, _events_rx) = tokio::sync::mpsc::channel(8);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let supervisor = Arc::new(tokio::sync::Mutex::new(AutoPeerDataListenerSupervisor::new(
            plan.clone(),
            Arc::clone(&state),
            dedupe,
            None,
            shutdown_rx,
        )));
        let runtime_status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);

        let err = plan
            .reconcile_link_local_addresses(
                Arc::clone(&state),
                Arc::clone(&supervisor),
                Some(&runtime_status),
                &events_tx,
                vec![AutoInterfaceDeviceCandidate {
                    ifname: "eth0".to_string(),
                    ipv6_addresses: vec!["fe80::2222".to_string()],
                }],
                |_| Err("missing interface index".to_string()),
            )
            .await
            .expect_err("restart should fail before state commit");

        assert!(err.contains("missing interface index"));
        assert_eq!(
            state
                .lock()
                .await
                .adopted_devices()
                .into_iter()
                .map(|device| device.link_local_address)
                .collect::<Vec<_>>(),
            vec!["fe80::1111".to_string()]
        );
        assert_eq!(
            runtime_status.to_json().get("link_local_update"),
            Some(&JsonValue::Null)
        );
        supervisor.lock().await.shutdown_all().await;
    }

    #[tokio::test]
    async fn auto_adopted_interface_add_failure_does_not_commit_state() {
        let plan = build_startup_plan_from_candidates(&auto_iface(), Vec::new())
            .expect("empty startup plan");
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (discovery_events_tx, _discovery_events_rx) = tokio::sync::mpsc::channel(8);
        let (data_events_tx, _data_events_rx) = tokio::sync::mpsc::channel(8);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let discovery_supervisor = Arc::new(tokio::sync::Mutex::new(
            AutoDiscoveryListenerSupervisor::new(
                plan.clone(),
                Arc::clone(&state),
                shutdown_rx.clone(),
            ),
        ));
        let data_supervisor = Arc::new(tokio::sync::Mutex::new(
            AutoPeerDataListenerSupervisor::new(
                plan.clone(),
                Arc::clone(&state),
                dedupe,
                None,
                shutdown_rx,
            ),
        ));
        let runtime_loop_handles = AutoInterfaceRuntimeLoopHandles {
            discovery_supervisor: Arc::clone(&discovery_supervisor),
            data_supervisor: Arc::clone(&data_supervisor),
            discovery_events: discovery_events_tx,
            data_events: data_events_tx,
        };

        let err = plan
            .reconcile_adopted_interface_add_remove(
                Arc::clone(&state),
                &runtime_loop_handles,
                vec![AutoInterfaceDeviceCandidate {
                    ifname: "eth0".to_string(),
                    ipv6_addresses: vec!["fe80::1111".to_string()],
                }],
                None,
                core::time::Duration::from_secs(8),
                |_| Err("missing interface index".to_string()),
            )
            .await
            .expect_err("add should fail before state commit");

        assert!(err.contains("missing interface index"));
        assert!(state.lock().await.adopted_devices().is_empty());
        assert_eq!(discovery_supervisor.lock().await.receive_loop_count(), 0);
        assert_eq!(data_supervisor.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn auto_adopted_interface_remove_commits_cleanup() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1111".to_string()],
            }],
        )
        .expect("startup plan");
        let state = Arc::new(tokio::sync::Mutex::new(plan.discovery_state()));
        state.lock().await.observe_discovery_packet(
            "fe80::aaaa",
            "eth0",
            core::time::Duration::from_secs(1),
        );
        let dedupe = Arc::new(tokio::sync::Mutex::new(AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        )));
        let (discovery_events_tx, _discovery_events_rx) = tokio::sync::mpsc::channel(8);
        let (data_events_tx, _data_events_rx) = tokio::sync::mpsc::channel(8);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let discovery_supervisor = Arc::new(tokio::sync::Mutex::new(
            AutoDiscoveryListenerSupervisor::new(
                plan.clone(),
                Arc::clone(&state),
                shutdown_rx.clone(),
            ),
        ));
        let data_supervisor = Arc::new(tokio::sync::Mutex::new(
            AutoPeerDataListenerSupervisor::new(
                plan.clone(),
                Arc::clone(&state),
                dedupe,
                None,
                shutdown_rx,
            ),
        ));
        let discovery_socket =
            tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind discovery socket");
        let discovery_bind_addr = discovery_socket.local_addr().expect("discovery bind addr");
        discovery_supervisor.lock().await.spawn_bound_listener(
            "eth0".to_string(),
            vec![AutoBoundDiscoverySocket {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "eth0".to_string(),
                bind_addr: discovery_bind_addr,
                multicast_group_addr: None,
                socket: discovery_socket,
            }],
            &discovery_events_tx,
        );
        let data_socket = Arc::new(
            tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind data socket"),
        );
        let data_bind_addr = data_socket.local_addr().expect("data bind addr");
        data_supervisor.lock().await.spawn_bound_socket(
            AutoBoundDataSocket {
                ifname: "eth0".to_string(),
                bind_addr: data_bind_addr,
                socket: data_socket,
            },
            &data_events_tx,
        );
        assert_eq!(discovery_supervisor.lock().await.receive_loop_count(), 1);
        assert_eq!(data_supervisor.lock().await.len(), 1);
        let runtime_loop_handles = AutoInterfaceRuntimeLoopHandles {
            discovery_supervisor: Arc::clone(&discovery_supervisor),
            data_supervisor: Arc::clone(&data_supervisor),
            discovery_events: discovery_events_tx,
            data_events: data_events_tx,
        };

        let applied = plan
            .reconcile_adopted_interface_add_remove(
                Arc::clone(&state),
                &runtime_loop_handles,
                Vec::new(),
                None,
                core::time::Duration::from_secs(12),
                |_| panic!("remove should not resolve scope ids"),
            )
            .await
            .expect("remove applies");

        assert_eq!(applied, 1);
        assert!(state.lock().await.adopted_devices().is_empty());
        assert!(state.lock().await.peer("fe80::aaaa").is_none());
        assert_eq!(discovery_supervisor.lock().await.receive_loop_count(), 0);
        assert_eq!(data_supervisor.lock().await.len(), 0);
    }

    #[test]
    fn auto_initial_peer_announce_sender_exposes_datagram_payloads() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut sent = Vec::new();

        let count = plan
            .send_initial_peer_announces(|datagram| {
                sent.push(datagram.clone());
                Ok(())
            })
            .expect("send planned datagrams");

        assert_eq!(count, 1);
        assert_eq!(sent[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(sent[0].destination_port, 48_555);
        assert_eq!(sent[0].payload, plan.peering_packets[0].token.to_vec());
    }

    #[test]
    fn auto_initial_peer_announce_sender_reports_destination_on_error() {
        let plan = build_startup_plan_from_candidates(
            &default_link_auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");

        let err = plan
            .send_initial_peer_announces(|_| Err("socket unavailable".to_string()))
            .expect_err("send failure should propagate");

        assert!(err.contains("send auto peer announce 1/1"));
        assert!(err.contains("[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0]:29716"));
        assert!(err.contains("socket unavailable"));
    }

    #[test]
    fn auto_repeat_peer_announce_job_uses_python_interval_after_initial_send() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        let mut sent = Vec::new();

        let initial = plan
            .run_multicast_peer_announce_job(&mut state, core::time::Duration::ZERO, |datagram| {
                sent.push(datagram.clone());
                Ok(())
            })
            .expect("initial multicast peer announce");
        let early = plan
            .run_multicast_peer_announce_job(
                &mut state,
                core::time::Duration::from_millis(1_599),
                |_| panic!("announce should not be due before the interval"),
            )
            .expect("early multicast peer announce check");
        let repeat = plan
            .run_multicast_peer_announce_job(
                &mut state,
                core::time::Duration::from_millis(1_600),
                |datagram| {
                    sent.push(datagram.clone());
                    Ok(())
                },
            )
            .expect("repeat multicast peer announce");

        assert_eq!(initial, 1);
        assert_eq!(early, 0);
        assert_eq!(repeat, 1);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(sent[0], sent[1]);
    }

    #[test]
    fn auto_peer_job_sends_reverse_announces_on_python_interval() {
        let plan = build_startup_plan_from_candidates(
            &auto_iface(),
            vec![AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec!["fe80::1234".to_string()],
            }],
        )
        .expect("startup plan");
        let mut state = plan.discovery_state();
        state.observe_discovery_packet("fe80::2222%eth0", "eth0", core::time::Duration::ZERO);

        let early = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(5_200), |_| {
                panic!("reverse announce should not be due at the interval boundary")
            })
            .expect("early peer job");
        let mut sent = Vec::new();
        let due = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(5_201), |datagram| {
                sent.push(datagram.clone());
                Ok(())
            })
            .expect("due peer job");
        let repeated = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(10_401), |_| {
                panic!("reverse announce should be marked sent")
            })
            .expect("repeated peer job");

        assert_eq!(early.reverse_peer_announce_count, 0);
        assert_eq!(due.reverse_peer_announce_count, 1);
        assert_eq!(repeated.reverse_peer_announce_count, 0);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].kind, AutoPeeringPacketKind::ReverseUnicast);
        assert_eq!(sent[0].destination_address, "fe80::2222%eth0");
        assert_eq!(sent[0].destination_port, 48_556);
        assert_eq!(sent[0].source_link_local_address, "fe80::1234");
    }

    #[test]
    fn auto_runtime_status_records_last_peer_job_lifecycle_summary() {
        let mut iface = auto_iface();
        iface.devices = Some(vec!["eth0".to_string(), "wlan0".to_string()]);
        let plan = build_startup_plan_from_candidates(
            &iface,
            vec![
                AutoInterfaceDeviceCandidate {
                    ifname: "eth0".to_string(),
                    ipv6_addresses: vec!["fe80::1111".to_string()],
                },
                AutoInterfaceDeviceCandidate {
                    ifname: "wlan0".to_string(),
                    ipv6_addresses: vec!["fe80::3333".to_string()],
                },
            ],
        )
        .expect("startup plan");
        let status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
        let mut state = plan.discovery_state();
        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::ZERO,
        );
        state.observe_discovery_packet("fe80::2222%eth0", "eth0", core::time::Duration::ZERO);

        let live = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(5_201), |_| Ok(()))
            .expect("live peer job");
        status.record_peer_job_summary(&live);
        let runtime = status.to_json();
        let peer_job = runtime.get("last_peer_job").expect("last peer job");
        assert_eq!(peer_job.get("peer_count_after").and_then(JsonValue::as_u64), Some(1));
        assert_eq!(
            peer_job.get("reverse_peer_announce_count").and_then(JsonValue::as_u64),
            Some(1)
        );
        assert_eq!(
            peer_job.get("missing_initial_echo_count").and_then(JsonValue::as_u64),
            Some(1)
        );

        let stale = plan
            .run_peer_job(&mut state, core::time::Duration::from_millis(22_001), |_| Ok(()))
            .expect("stale peer job");
        status.record_peer_job_summary(&stale);
        let runtime = status.to_json();
        let peer_job = runtime.get("last_peer_job").expect("last peer job");
        assert_eq!(peer_job.get("expired_peer_count").and_then(JsonValue::as_u64), Some(1));
        assert_eq!(peer_job.get("peer_count_after").and_then(JsonValue::as_u64), Some(0));
    }

    #[test]
    fn auto_discovery_socket_bind_targets_format_unicast_and_multicast_scopes() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1234".to_string(),
            unicast_bind_address: "fe80::1234%eth0".to_string(),
            unicast_bind_port: 29_717,
            multicast_group_address: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            multicast_bind_address: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0".to_string(),
            multicast_bind_port: 29_716,
        });

        let targets = plan.discovery_socket_bind_targets();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].kind, AutoDiscoverySocketKind::Unicast);
        assert_eq!(targets[0].display_bind_addr(), "[fe80::1234%eth0]:29717");
        assert_eq!(targets[1].kind, AutoDiscoverySocketKind::Multicast);
        assert_eq!(
            targets[1].display_bind_addr(),
            "[ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0]:29716"
        );
        assert_eq!(
            targets[1].multicast_group_host.as_deref(),
            Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1")
        );
    }

    #[test]
    fn auto_data_socket_bind_targets_format_scoped_listener() {
        let plan = plan_with_data_listener(AutoDataListenerBinding {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1234".to_string(),
            bind_address: "fe80::1234%eth0".to_string(),
            bind_port: 42_671,
        });

        let targets = plan.data_socket_bind_targets();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].ifname, "eth0");
        assert_eq!(targets[0].display_bind_addr(), "[fe80::1234%eth0]:42671");
        assert_eq!(targets[0].scope_ifname.as_deref(), Some("eth0"));
    }

    #[test]
    fn auto_discovery_socket_bind_targets_use_unspecified_for_windows_empty_hosts() {
        let plan = plan_with_discovery_listener(AutoDiscoveryListenerBinding {
            ifname: "Ethernet".to_string(),
            link_local_address: "fe80::1234".to_string(),
            unicast_bind_address: String::new(),
            unicast_bind_port: 29_717,
            multicast_group_address: "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1".to_string(),
            multicast_bind_address: String::new(),
            multicast_bind_port: 29_716,
        });

        let targets = plan.discovery_socket_bind_targets();

        assert_eq!(targets[0].display_bind_addr(), "[::]:29717");
        assert_eq!(targets[0].scope_ifname, None);
        assert_eq!(targets[1].display_bind_addr(), "[::]:29716");
        assert_eq!(targets[1].scope_ifname, None);
    }
