    #[test]
    fn discovery_state_records_local_multicast_echo_without_peer() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        let event = state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_secs(7),
        );

        assert_eq!(event, AutoDiscoveryEvent::LocalMulticastEcho { ifname: "eth0".to_string() });
        assert_eq!(state.peer_count(), 0);
        assert_eq!(state.last_multicast_echo("eth0"), Some(core::time::Duration::from_secs(7)));
        assert_eq!(state.initial_multicast_echo("eth0"), Some(core::time::Duration::from_secs(7)));
    }

    #[test]
    fn discovery_state_observes_remote_peer_when_not_local_echo() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        let event = state.observe_discovery_packet(
            "fe80::2222%eth0",
            "eth0",
            core::time::Duration::from_secs(7),
        );

        assert_eq!(event, AutoDiscoveryEvent::Peer(AutoPeerEvent::Added));
        assert_eq!(state.peer_count(), 1);
        assert!(state.peer("fe80::2222").is_some());
        assert_eq!(state.last_multicast_echo("eth0"), None);
    }

    #[test]
    fn discovery_state_rejects_unauthenticated_discovery_packet() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        let err = state
            .observe_authenticated_discovery_packet(
                &[0xAA; 32],
                b"reticulum",
                "fe80::2222%eth0",
                "eth0",
                core::time::Duration::from_secs(7),
            )
            .expect_err("bad discovery token must be rejected");

        assert_eq!(err, AutoDiscoveryRejectReason::InvalidToken);
        assert_eq!(state.peer_count(), 0);
        assert_eq!(state.last_multicast_echo("eth0"), None);
    }

    #[test]
    fn discovery_state_accepts_authenticated_remote_peer_packet() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        let token = peering_token(b"reticulum", "fe80::2222%eth0");

        let event = state
            .observe_authenticated_discovery_packet(
                &token,
                b"reticulum",
                "fe80::2222%eth0",
                "eth0",
                core::time::Duration::from_secs(7),
            )
            .expect("valid discovery token");

        assert_eq!(event, AutoDiscoveryEvent::Peer(AutoPeerEvent::Added));
        assert!(state.peer("fe80::2222").is_some());
    }

    #[test]
    fn discovery_state_accepts_authenticated_packet_with_suffix_like_python() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        let mut packet = peering_token(b"reticulum", "fe80::2222%eth0").to_vec();
        packet.extend_from_slice(b"ignored suffix");

        let event = state
            .observe_authenticated_discovery_packet(
                &packet,
                b"reticulum",
                "fe80::2222%eth0",
                "eth0",
                core::time::Duration::from_secs(7),
            )
            .expect("valid token prefix");

        assert_eq!(event, AutoDiscoveryEvent::Peer(AutoPeerEvent::Added));
    }

    #[test]
    fn discovery_state_tracks_python_multicast_echo_timeout_boundary() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );
        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_secs(10),
        );

        let events = state.update_multicast_echo_timeouts(
            core::time::Duration::from_millis(16_500),
            core::time::Duration::from_millis(6_500),
        );
        assert!(events.is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(false));

        let events = state.update_multicast_echo_timeouts(
            core::time::Duration::from_millis(16_501),
            core::time::Duration::from_millis(6_500),
        );
        assert_eq!(
            events,
            vec![AutoMulticastCarrierEvent::CarrierLost { ifname: "eth0".to_string() }]
        );
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(true));
    }

    #[test]
    fn discovery_state_recovers_carrier_after_local_echo_returns() {
        let mut state = AutoDiscoveryState::new(
            vec![AutoInterfaceAdoptedDevice {
                ifname: "eth0".to_string(),
                link_local_address: "fe80::1111".to_string(),
            }],
            core::time::Duration::from_secs(22),
            core::time::Duration::from_millis(5_200),
        );

        assert!(state
            .update_multicast_echo_timeouts(
                core::time::Duration::from_millis(6_501),
                core::time::Duration::from_millis(6_500),
            )
            .is_empty());
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(true));

        state.observe_discovery_packet(
            "fe80::1111%eth0",
            "eth0",
            core::time::Duration::from_millis(7_000),
        );
        let events = state.update_multicast_echo_timeouts(
            core::time::Duration::from_millis(7_000),
            core::time::Duration::from_millis(6_500),
        );

        assert_eq!(
            events,
            vec![AutoMulticastCarrierEvent::CarrierRecovered { ifname: "eth0".to_string() }]
        );
        assert_eq!(state.multicast_echo_timed_out("eth0"), Some(false));
    }

    #[test]
    fn inbound_deduplicator_matches_python_multi_interface_ttl() {
        let mut dedupe =
            AutoInboundPacketDeduplicator::new(48, core::time::Duration::from_millis(750));

        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_000)));
        assert!(!dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_749)));
        assert!(dedupe.should_accept(b"packet", core::time::Duration::from_millis(1_751)));
    }

    #[test]
    fn inbound_deduplicator_retains_python_window_length() {
        let mut dedupe =
            AutoInboundPacketDeduplicator::new(48, core::time::Duration::from_millis(750));
        for i in 0..48 {
            assert!(dedupe.should_accept(&[i], core::time::Duration::from_secs(1)));
        }

        assert!(!dedupe.should_accept(&[0], core::time::Duration::from_millis(1_100)));
        assert!(dedupe.should_accept(&[48], core::time::Duration::from_millis(1_100)));
        assert!(dedupe.should_accept(&[0], core::time::Duration::from_millis(1_200)));
    }
