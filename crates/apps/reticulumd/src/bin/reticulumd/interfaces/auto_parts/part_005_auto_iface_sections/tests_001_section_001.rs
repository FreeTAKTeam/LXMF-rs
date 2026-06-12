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
