    #[test]
    fn peer_job_plan_matches_python_reverse_announce_and_initial_echo_checks() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![
            AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            },
            AutoInterfaceAdoptedDevice {
                ifname: "wlan0".to_string(),
                link_local_address: "fe80::3333".to_string(),
            },
        ];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet("fe80::1111%eth0", "eth0", core::time::Duration::ZERO);
        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::ZERO);

        let plan = state.peer_job_plan(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(5_201),
        );

        assert!(plan.expired_peers.is_empty());
        assert_eq!(plan.missing_initial_echo_interfaces, vec!["wlan0"]);
        assert_eq!(plan.reverse_peering_packets.len(), 1);
        assert_eq!(plan.reverse_peering_packets[0].kind, AutoPeeringPacketKind::ReverseUnicast);
        assert_eq!(plan.reverse_peering_packets[0].destination_address, "fe80::2222%eth0");
        assert_eq!(plan.reverse_peering_packets[0].destination_port, 29_717);
    }

    #[test]
    fn peer_job_plan_expires_peers_before_reverse_announces_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::ZERO);

        let plan = state.peer_job_plan(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(22_001),
        );

        assert_eq!(plan.expired_peers.len(), 1);
        assert_eq!(plan.expired_peers[0].address, "fe80::2222");
        assert!(plan.reverse_peering_packets.is_empty());
    }

    #[test]
    fn run_peer_job_marks_reverse_announced_and_updates_carrier_state_like_python() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_millis(1_000),
        );
        state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_millis(1_000),
        );
        let initial_run = state.run_peer_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(1_000),
            timing.multicast_echo_timeout,
        );
        assert!(initial_run.carrier_events.is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(false));

        let run = state.run_peer_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(7_501),
            timing.multicast_echo_timeout,
        );

        assert!(run.expired_peers.is_empty());
        assert_eq!(run.reverse_peering_packets.len(), 1);
        assert_eq!(
            run.carrier_events,
            vec![AutoMulticastCarrierEvent::CarrierLost { ifname: "eth0".to_string() }]
        );
        assert!(state.reverse_announces_due(core::time::Duration::from_millis(12_701)).is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(true));
    }

    #[test]
    fn run_peer_job_expires_stale_peers_before_marking_reverse_announces() {
        let config = AutoInterfaceConfig::default();
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let adopted_devices = vec![AutoInterfaceAdoptedDevice {
            ifname: "eth0".to_string(),
            link_local_address: "fe80::1111".to_string(),
        }];
        let mut state = AutoDiscoveryState::from_timing(adopted_devices.clone(), timing);

        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::ZERO);

        let run = state.run_peer_job(
            &config,
            &adopted_devices,
            core::time::Duration::from_millis(22_001),
            timing.multicast_echo_timeout,
        );

        assert_eq!(run.expired_peers.len(), 1);
        assert_eq!(run.expired_peers[0].address, "fe80::2222");
        assert!(run.reverse_peering_packets.is_empty());
        assert!(state.peer("fe80::2222").is_none());
    }

    #[test]
    fn peering_token_matches_python_auto_interface() {
        let token = peering_token(b"reticulum", "fe80::1234:abcd");

        assert_eq!(
            hex::encode(token),
            "2158465c9c7ece3cc433c698231ebd4304b7f278e352c769426ade2b0ebecff0"
        );
        assert!(verify_peering_token(&token, b"reticulum", "fe80::1234:abcd"));
        assert!(!verify_peering_token(&token, b"reticulum", "fe80::beef"));
    }

    #[test]
    fn peering_token_verification_matches_python_payload_slicing() {
        let token = peering_token(b"reticulum", "fe80::1234:abcd");
        let mut payload = token.to_vec();
        payload.extend_from_slice(b"ignored suffix");

        assert!(verify_peering_token(&payload, b"reticulum", "fe80::1234:abcd"));
        assert!(!verify_peering_token(&payload[..31], b"reticulum", "fe80::1234:abcd"));
    }

    #[test]
    fn descopes_link_local_addresses_like_python_auto_interface() {
        assert_eq!(descope_link_local("fe80::1234%eth0"), "fe80::1234");
        assert_eq!(descope_link_local("fe80:abcd::1234"), "fe80::1234");
        assert_eq!(descope_link_local("2001:db8::1"), "2001:db8::1");
    }

    #[test]
    fn timing_defaults_match_python_auto_interface() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);

        assert_eq!(timing.peering_timeout, core::time::Duration::from_secs(22));
        assert_eq!(timing.announce_interval, core::time::Duration::from_millis(1_600));
        assert_eq!(timing.peer_job_interval, core::time::Duration::from_secs(4));
        assert_eq!(timing.multicast_echo_timeout, core::time::Duration::from_millis(6_500));
        assert_eq!(timing.reverse_peering_interval, core::time::Duration::from_millis(5_200));
        assert_eq!(timing.initial_peering_wait, core::time::Duration::from_millis(1_920));
        assert_eq!(timing.multi_interface_dedupe_ttl, core::time::Duration::from_millis(750));
        assert_eq!(timing.multi_interface_dedupe_len, 48);
    }

    #[test]
    fn timing_applies_python_android_peering_multiplier() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Android);

        assert_eq!(timing.peering_timeout, core::time::Duration::from_millis(27_500));
        assert_eq!(timing.reverse_peering_interval, core::time::Duration::from_millis(5_200));
    }

    #[test]
    fn discovery_state_from_timing_uses_python_peer_intervals() {
        let mut state = AutoDiscoveryState::from_timing(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Android),
        );
        state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_secs(0),
        );

        assert!(state.reverse_announces_due(core::time::Duration::from_millis(5_200)).is_empty());
        let due = state.reverse_announces_due(core::time::Duration::from_millis(5_201));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].address, "fe80::2222");

        assert!(state.expire_stale_peers(core::time::Duration::from_millis(27_500)).is_empty());
        let expired = state.expire_stale_peers(core::time::Duration::from_millis(27_501));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].address, "fe80::2222");
    }

    #[test]
    fn inbound_deduplicator_from_timing_uses_python_window() {
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(
            AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other),
        );

        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_000)));
        assert!(!dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_749)));
        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_751)));
    }

    #[test]
    fn spawned_peer_inbound_accepts_known_peer_and_refreshes_it_like_python() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let mut state = AutoDiscoveryState::from_timing(Vec::new(), timing);
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(timing);
        state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_secs(1),
        );

        let decision = state.handle_spawned_peer_inbound(
            &mut dedupe,
            "fe80::2222%eth0",
            b"packet",
            core::time::Duration::from_secs(2),
        );

        assert_eq!(
            decision,
            AutoPeerInboundDecision::Accepted {
                peer: AutoPeer {
                    address: "fe80::2222".to_string(),
                    ifname: "eth0".to_string(),
                    last_heard_at: core::time::Duration::from_secs(2),
                    last_outbound_at: core::time::Duration::from_secs(1),
                }
            }
        );
        assert_eq!(
            state.peer("fe80::2222").expect("peer").last_heard_at,
            core::time::Duration::from_secs(2)
        );
        assert_eq!(dedupe.len(), 1);
    }

    #[test]
    fn spawned_peer_inbound_suppresses_duplicate_without_refreshing_peer() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let mut state = AutoDiscoveryState::from_timing(Vec::new(), timing);
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(timing);
        state.observe_discovery_packet("fe80::2222", "eth0", core::time::Duration::from_secs(1));

        assert!(matches!(
            state.handle_spawned_peer_inbound(
                &mut dedupe,
                "fe80::2222",
                b"packet",
                core::time::Duration::from_millis(2_000),
            ),
            AutoPeerInboundDecision::Accepted { .. }
        ));
        let duplicate = state.handle_spawned_peer_inbound(
            &mut dedupe,
            "fe80::2222",
            b"packet",
            core::time::Duration::from_millis(2_500),
        );

        assert_eq!(duplicate, AutoPeerInboundDecision::Duplicate);
        assert_eq!(
            state.peer("fe80::2222").expect("peer").last_heard_at,
            core::time::Duration::from_millis(2_000)
        );
    }

    #[test]
    fn spawned_peer_inbound_rejects_unknown_peer_without_touching_dedupe() {
        let timing = AutoInterfaceTiming::for_platform(AutoInterfacePlatform::Other);
        let mut state = AutoDiscoveryState::from_timing(Vec::new(), timing);
        let mut dedupe = AutoInboundPacketDeduplicator::from_timing(timing);

        let decision = state.handle_spawned_peer_inbound(
            &mut dedupe,
            "fe80::4444",
            b"packet",
            core::time::Duration::from_secs(2),
        );

        assert_eq!(decision, AutoPeerInboundDecision::UnknownPeer);
        assert!(state.peer("fe80::4444").is_none());
        assert_eq!(dedupe.len(), 0);
    }

    #[test]
    fn peer_table_adds_new_peer_and_refreshes_existing_like_python() {
        let mut peers = AutoPeerTable::new(
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        assert_eq!(
            peers.observe_peer("fe80::1", "eth0", core::time::Duration::from_secs(10)),
            AutoPeerEvent::Added
        );
        assert_eq!(peers.len(), 1);

        assert_eq!(
            peers.observe_peer("fe80::1", "wlan0", core::time::Duration::from_secs(12)),
            AutoPeerEvent::Refreshed
        );
        let peer = peers.peer("fe80::1").expect("peer");
        assert_eq!(peer.ifname, "eth0");
        assert_eq!(peer.last_heard_at, core::time::Duration::from_secs(12));
    }

    #[test]
    fn peer_table_expires_stale_peers_after_python_timeout() {
        let mut peers = AutoPeerTable::new(
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        peers.observe_peer("fe80::1", "eth0", core::time::Duration::from_secs(0));

        assert!(peers.expire_stale(core::time::Duration::from_secs(22)).is_empty());
        let expired = peers.expire_stale(core::time::Duration::from_secs(23));

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].address, "fe80::1");
        assert_eq!(peers.len(), 0);
    }

    #[test]
    fn peer_table_tracks_reverse_announce_due_times() {
        let mut peers = AutoPeerTable::new(
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        peers.observe_peer("fe80::1", "eth0", core::time::Duration::from_secs(10));

        assert!(peers.reverse_announces_due(core::time::Duration::from_millis(15_200)).is_empty());
        let due = peers.reverse_announces_due(core::time::Duration::from_millis(15_201));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].address, "fe80::1");

        peers.mark_reverse_announced("fe80::1", core::time::Duration::from_millis(15_201));
        assert!(peers.reverse_announces_due(core::time::Duration::from_millis(20_401)).is_empty());
    }

    #[test]
    fn device_filter_matches_python_allow_and_ignore_order() {
        let filter = AutoInterfaceDeviceFilter {
            allowed: vec!["awdl0".to_string()],
            ignored: vec!["eth0".to_string()],
        };

        assert!(filter.should_adopt("awdl0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("eth0", AutoInterfacePlatform::Other));
        assert!(!filter.should_adopt("en0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("lo0", AutoInterfacePlatform::Darwin));
    }

    #[test]
    fn device_filter_matches_python_platform_defaults() {
        let filter = AutoInterfaceDeviceFilter::default();

        assert!(!filter.should_adopt("awdl0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("llw0", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("en5", AutoInterfacePlatform::Darwin));
        assert!(!filter.should_adopt("lo0", AutoInterfacePlatform::Other));
        assert!(!filter.should_adopt("rmnet0", AutoInterfacePlatform::Android));
        assert!(filter.should_adopt("eth0", AutoInterfacePlatform::Other));
    }

    #[test]
    fn adopted_devices_select_python_link_local_addresses() {
        let filter = AutoInterfaceDeviceFilter {
            allowed: vec!["eth0".to_string(), "wlan0".to_string(), "eth1".to_string()],
            ignored: vec![],
        };
        let candidates = vec![
            AutoInterfaceDeviceCandidate {
                ifname: "eth0".to_string(),
                ipv6_addresses: vec![
                    "2001:db8::1".to_string(),
                    "fe80::1111%eth0".to_string(),
                    "fe80:abcd::2222".to_string(),
                ],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "wlan0".to_string(),
                ipv6_addresses: vec!["2001:db8::2".to_string()],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "eth1".to_string(),
                ipv6_addresses: vec!["fe80::3333%eth1".to_string()],
            },
        ];

        let adopted = filter.adopt_devices(&candidates, AutoInterfacePlatform::Other);

        assert_eq!(adopted.len(), 2);
        assert_eq!(adopted[0].ifname, "eth0");
        assert_eq!(adopted[0].link_local_address, "fe80::2222");
        assert_eq!(adopted[1].ifname, "eth1");
        assert_eq!(adopted[1].link_local_address, "fe80::3333");
    }

    #[test]
    fn adopted_devices_apply_platform_filter_before_link_local_selection() {
        let filter = AutoInterfaceDeviceFilter::default();
        let candidates = vec![
            AutoInterfaceDeviceCandidate {
                ifname: "awdl0".to_string(),
                ipv6_addresses: vec!["fe80::1111%awdl0".to_string()],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "en0".to_string(),
                ipv6_addresses: vec!["fe80::2222%en0".to_string()],
            },
        ];

        let adopted = filter.adopt_devices(&candidates, AutoInterfacePlatform::Darwin);

        assert_eq!(adopted.len(), 1);
        assert_eq!(adopted[0].ifname, "en0");
        assert_eq!(adopted[0].link_local_address, "fe80::2222");
    }
