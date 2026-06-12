    use super::{
        descope_link_local, peering_token, verify_peering_token, AutoDataListenerBinding,
        AutoDiscoveryEvent, AutoDiscoveryRejectReason, AutoDiscoveryScope, AutoDiscoveryState,
        AutoInboundPacketDeduplicator, AutoInterfaceAdoptedDevice, AutoInterfaceConfig,
        AutoInterfaceDeviceCandidate, AutoInterfaceDeviceFilter, AutoInterfacePlatform,
        AutoInterfaceTiming, AutoLinkLocalAddressUpdate, AutoMulticastCarrierEvent, AutoPeer,
        AutoPeerEvent, AutoPeerInboundDecision, AutoPeerTable, AutoPeeringPacketKind,
        AutoRuntimeEvent, AutoRuntimeState, MulticastAddressType,
    };

    #[test]
    fn default_multicast_discovery_address_matches_python_auto_interface() {
        let config = AutoInterfaceConfig::default();

        assert_eq!(config.multicast_discovery_address(), "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
    }

    #[test]
    fn custom_multicast_discovery_address_matches_python_auto_interface() {
        let config = AutoInterfaceConfig {
            group_id: "field-net".to_string(),
            discovery_scope: AutoDiscoveryScope::Global,
            multicast_address_type: MulticastAddressType::Permanent,
            discovery_port: 48_555,
            data_port: 49_555,
        };

        assert_eq!(config.multicast_discovery_address(), "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d");
    }

    #[test]
    fn multicast_peering_packet_matches_python_peer_announce() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%eth0".to_string(),
        };

        let packet = config.multicast_peering_packet(&adopted);

        assert_eq!(packet.kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(packet.ifname, "eth0");
        assert_eq!(packet.source_link_local_address, "fe80::1111");
        assert_eq!(packet.destination_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
        assert_eq!(packet.destination_port, 29_716);
        assert_eq!(packet.token, peering_token(b"reticulum", "fe80::1111"));
    }

    #[test]
    fn multicast_announce_job_sends_immediately_like_python_announce_handler() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111%eth0".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::2222%wlan0".to_string(),
            },
        ];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        let packets = state.run_multicast_announce_job(
            &config,
            &adopted_devices,
            core::time::Duration::ZERO,
            timing.announce_interval,
        );

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].kind, AutoPeeringPacketKind::Multicast);
        assert_eq!(packets[0].ifname, "eth0");
        assert_eq!(packets[0].destination_port, 29_716);
        assert_eq!(packets[1].ifname, "wlan0");
    }

    #[test]
    fn multicast_announce_job_respects_python_announce_interval() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        assert_eq!(
            state
                .run_multicast_announce_job(
                    &config,
                    &adopted_devices,
                    core::time::Duration::ZERO,
                    timing.announce_interval,
                )
                .len(),
            1
        );
        assert!(state
            .run_multicast_announce_job(
                &config,
                &adopted_devices,
                core::time::Duration::from_millis(1_599),
                timing.announce_interval,
            )
            .is_empty());

        let packets = state.run_multicast_announce_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(1_600),
            timing.announce_interval,
        );

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].destination_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
    }

    #[test]
    fn reverse_peering_packet_matches_python_reverse_announce() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%eth0".to_string(),
        };

        let packet = config.reverse_peering_packet(&adopted, "fe80::2222");

        assert_eq!(packet.kind, AutoPeeringPacketKind::ReverseUnicast);
        assert_eq!(packet.ifname, "eth0");
        assert_eq!(packet.source_link_local_address, "fe80::1111");
        assert_eq!(packet.destination_address, "fe80::2222%eth0");
        assert_eq!(packet.destination_port, 29_717);
        assert_eq!(packet.token, peering_token(b"reticulum", "fe80::1111"));
    }

    #[test]
    fn peer_data_target_matches_python_spawned_peer_delivery() {
        let config = AutoInterfaceConfig::default();
        let peer = AutoPeer {
            address: "fe80::2222%ignored".to_string(),
            ifname: "eth0".to_string(),
            last_heard_at: core::time::Duration::from_secs(1),
            last_outbound_at: core::time::Duration::from_secs(1),
        };

        let target = config.peer_data_target(&peer);

        assert_eq!(target.ifname, "eth0");
        assert_eq!(target.peer_address, "fe80::2222");
        assert_eq!(target.destination_address, "fe80::2222%eth0");
        assert_eq!(target.destination_port, 42_671);
    }

    #[test]
    fn peer_data_target_uses_configured_data_port() {
        let config = AutoInterfaceConfig { data_port: 49_555, ..AutoInterfaceConfig::default() };
        let peer = AutoPeer {
            address: "fe80::3333".to_string(),
            ifname: "wlan0".to_string(),
            last_heard_at: core::time::Duration::from_secs(1),
            last_outbound_at: core::time::Duration::from_secs(1),
        };

        let target = config.peer_data_target(&peer);

        assert_eq!(target.destination_address, "fe80::3333%wlan0");
        assert_eq!(target.destination_port, 49_555);
    }

    #[test]
    fn data_listener_binding_matches_python_final_init_udp_server_target() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%ignored".to_string(),
        };

        let binding = config.data_listener_binding(&adopted);

        assert_eq!(binding.ifname, "eth0");
        assert_eq!(binding.link_local_address, "fe80::1111");
        assert_eq!(binding.bind_address, "fe80::1111%eth0");
        assert_eq!(binding.bind_port, 42_671);
    }

    #[test]
    fn data_listener_bindings_use_configured_data_port_and_preserve_adopted_order() {
        let config = AutoInterfaceConfig { data_port: 49_555, ..AutoInterfaceConfig::default() };
        let adopted = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::2222%wlan0".to_string(),
            },
        ];

        let bindings = config.data_listener_bindings(&adopted);

        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].bind_address, "fe80::1111%eth0");
        assert_eq!(bindings[0].bind_port, 49_555);
        assert_eq!(bindings[1].bind_address, "fe80::2222%wlan0");
        assert_eq!(bindings[1].bind_port, 49_555);
    }

    #[test]
    fn discovery_listener_binding_matches_python_non_windows_startup_targets() {
        let config = AutoInterfaceConfig::default();
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111%ignored".to_string(),
        };

        let binding = config.discovery_listener_binding(&adopted, AutoInterfacePlatform::Other);

        assert_eq!(binding.ifname, "eth0");
        assert_eq!(binding.link_local_address, "fe80::1111");
        assert_eq!(binding.unicast_bind_address, "fe80::1111%eth0");
        assert_eq!(binding.unicast_bind_port, 29_717);
        assert_eq!(binding.multicast_group_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
        assert_eq!(binding.multicast_bind_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1%eth0");
        assert_eq!(binding.multicast_bind_port, 29_716);
    }

    #[test]
    fn discovery_listener_binding_matches_python_global_scope_and_windows_bind_targets() {
        let global_config = AutoInterfaceConfig {
            discovery_scope: AutoDiscoveryScope::Global,
            ..AutoInterfaceConfig::default()
        };
        let adopted = AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        };

        let global =
            global_config.discovery_listener_binding(&adopted, AutoInterfacePlatform::Other);
        assert_eq!(global.multicast_bind_address, "ff1e:0:d70b:fb1c:16e4:5e39:485e:31e1");

        let windows = AutoInterfaceConfig::default()
            .discovery_listener_binding(&adopted, AutoInterfacePlatform::Windows);
        assert_eq!(windows.unicast_bind_address, "");
        assert_eq!(windows.unicast_bind_port, 29_717);
        assert_eq!(windows.multicast_bind_address, "");
        assert_eq!(windows.multicast_bind_port, 29_716);
        assert_eq!(windows.multicast_group_address, "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
    }

    #[test]
    fn startup_plan_aggregates_python_final_init_runtime_targets() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111%eth0".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::2222".to_string(),
            },
        ];

        let plan = config.startup_plan(&adopted, AutoInterfacePlatform::Other, timing);

        assert_eq!(plan.initial_peering_wait, core::time::Duration::from_millis(1_920));
        assert_eq!(plan.peer_job_interval, core::time::Duration::from_secs(4));
        assert_eq!(plan.discovery_listeners.len(), 2);
        assert_eq!(plan.discovery_listeners[0].ifname, "eth0");
        assert_eq!(plan.discovery_listeners[1].unicast_bind_address, "fe80::2222%wlan0");
        assert_eq!(plan.data_listeners.len(), 2);
        assert_eq!(plan.data_listeners[0].bind_address, "fe80::1111%eth0");
        assert_eq!(plan.data_listeners[1].bind_address, "fe80::2222%wlan0");
    }

    #[test]
    fn startup_plan_carries_windows_discovery_bindings_but_normal_data_listeners() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Windows);
        let adopted = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];

        let plan = config.startup_plan(&adopted, AutoInterfacePlatform::Windows, timing);

        assert_eq!(plan.discovery_listeners[0].unicast_bind_address, "");
        assert_eq!(plan.discovery_listeners[0].multicast_bind_address, "");
        assert_eq!(plan.data_listeners[0].bind_address, "fe80::1111%eth0");
        assert_eq!(plan.initial_peering_wait, core::time::Duration::from_millis(1_920));
    }

    #[test]
    fn runtime_state_gates_discovery_until_python_final_init_wait_completes() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);

        assert!(!runtime.online);
        assert!(!runtime.final_init_done);
        assert!(!runtime.can_process_discovery_packets());
        assert_eq!(runtime.advance(core::time::Duration::from_millis(1_919)), None);
        assert!(!runtime.can_process_discovery_packets());

        assert_eq!(
            runtime.advance(core::time::Duration::from_millis(1_920)),
            Some(AutoRuntimeEvent::FinalInitCompleted)
        );
        assert!(runtime.online);
        assert!(runtime.final_init_done);
        assert!(runtime.can_process_discovery_packets());
        assert_eq!(runtime.advance(core::time::Duration::from_millis(3_000)), None);
    }

    #[test]
    fn runtime_state_gates_spawned_peer_inbound_on_online_state_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);

        assert!(!runtime.can_process_spawned_peer_inbound());
        runtime.advance(core::time::Duration::from_millis(1_920));
        assert!(runtime.can_process_spawned_peer_inbound());

        runtime.detach();

        assert!(!runtime.online);
        assert!(runtime.final_init_done);
        assert!(!runtime.can_process_spawned_peer_inbound());
        assert!(runtime.can_process_discovery_packets());
    }

    #[test]
    fn runtime_state_records_multicast_carrier_transitions_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);

        assert!(!runtime.carrier_changed);
        assert!(!runtime.record_carrier_events(&[]));
        assert!(!runtime.carrier_changed);

        assert!(runtime.record_carrier_events(&[AutoMulticastCarrierEvent::CarrierLost {
            ifname: "eth0".to_string(),
        }]));
        assert!(runtime.carrier_changed);

        runtime.clear_carrier_changed();

        assert!(!runtime.carrier_changed);
    }

    #[test]
    fn runtime_state_records_link_local_replacement_as_carrier_change_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let plan = config.startup_plan(&[], AutoInterfacePlatform::Other, timing);
        let mut runtime = AutoRuntimeState::from_startup_plan(&plan, core::time::Duration::ZERO);
        let update = AutoLinkLocalAddressUpdate {
            ifname: "eth0".to_string(),
            old_link_local_address: "fe80::1111".to_string(),
            new_link_local_address: "fe80::2222".to_string(),
            listener_binding: AutoDataListenerBinding {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::2222".to_string(),
                bind_address: "fe80::2222%eth0".to_string(),
                bind_port: config.data_port,
            },
        };

        assert!(!runtime.carrier_changed);
        assert!(runtime.record_link_local_update(Some(&update)));
        assert!(runtime.carrier_changed);

        runtime.clear_carrier_changed();

        assert!(!runtime.record_link_local_update(None));
        assert!(!runtime.carrier_changed);
    }

    #[test]
    fn link_local_update_replaces_adopted_address_and_plans_listener_restart_like_python() {
        let config = AutoInterfaceConfig::default();
        let mut state = AutoDiscoveryState::from_timing(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        let update = state.update_adopted_link_local_address(&config, "eth0", "fe80::2222%eth0");

        assert_eq!(
            update,
            Some(AutoLinkLocalAddressUpdate {
                ifname: "eth0".to_string(),
                old_link_local_address: "fe80::1111".to_string(),
                new_link_local_address: "fe80::2222".to_string(),
                listener_binding: AutoDataListenerBinding {
                    ifname: "eth0".to_string(),
                    link_local_address: "fe80::2222".to_string(),
                    bind_address: "fe80::2222%eth0".to_string(),
                    bind_port: 42_671,
                },
            })
        );
        assert_eq!(
            state.observe_discovery_packet(
                "fe80::2222",
                "eth0",
                core::time::Duration::from_secs(3),
            ),
            AutoDiscoveryEvent::LocalMulticastEcho { ifname: "eth0".to_string() }
        );
        assert_eq!(
            state.observe_discovery_packet(
                "fe80::1111",
                "eth0",
                core::time::Duration::from_secs(4),
            ),
            AutoDiscoveryEvent::Peer(AutoPeerEvent::Added)
        );
    }

    #[test]
    fn link_local_update_is_noop_for_same_or_unknown_interface() {
        let config = AutoInterfaceConfig::default();
        let mut state = AutoDiscoveryState::from_timing(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        assert_eq!(
            state.update_adopted_link_local_address(&config, "eth0", "fe80::1111%eth0"),
            None
        );
        assert_eq!(state.update_adopted_link_local_address(&config, "wlan0", "fe80::2222"), None);
    }
