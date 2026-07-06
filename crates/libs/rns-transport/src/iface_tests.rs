#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::PacketType;

    include!("iface_tests_parts/closed_tx_queue_cleanup.rs");

    #[test]
    fn new_channel_defaults_to_unicast_role() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert_eq!(mgr.role(channel.address()), Some(IfaceRole::Unicast));
        assert_eq!(mgr.mode(channel.address()), Some(InterfaceMode::Full));
    }

    #[test]
    fn new_channel_with_role_records_multicast_tag() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel_with_role(16, IfaceRole::Multicast);
        assert_eq!(mgr.role(channel.address()), Some(IfaceRole::Multicast));
        assert!(mgr.has_role(IfaceRole::Multicast));
    }

    #[test]
    fn new_channel_with_mode_records_mode() {
        let mut mgr = InterfaceManager::new(16);
        let channel =
            mgr.new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Boundary);
        assert_eq!(mgr.mode(channel.address()), Some(InterfaceMode::Boundary));
    }

    #[test]
    fn new_channel_defaults_to_outgoing_enabled() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert_eq!(mgr.outgoing(channel.address()), Some(true));
    }

    #[test]
    fn set_outgoing_updates_registered_iface() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert!(mgr.set_outgoing(*channel.address(), false));
        assert_eq!(mgr.outgoing(channel.address()), Some(false));
    }

    #[test]
    fn new_channel_defaults_to_python_style_announce_pacing() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert_eq!(mgr.announce_pacing(channel.address()), Some((62_500, 2)));
    }

    #[test]
    fn new_context_records_configured_interface_mtu() {
        let mut mgr = InterfaceManager::new(16);
        let context = mgr.new_context(crate::iface::kiss::KissInterface::new("ttyUSB0", 57_600).with_mtu(220));

        assert_eq!(mgr.mtu(context.channel.address()), Some(220));
    }

    #[tokio::test]
    async fn direct_packet_over_configured_mtu_is_not_queued() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_mode_mtu(16, IfaceRole::Unicast, InterfaceMode::Full, 20)
            .tx_channel;
        let iface = mgr.ifaces[0].address;
        let packet = packet_with_wire_len(21);

        let trace = mgr.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;

        assert_eq!(trace.matched_ifaces, 1);
        assert_eq!(trace.sent_ifaces, 0);
        assert_eq!(trace.failed_ifaces, 1);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn direct_packet_at_configured_mtu_is_queued() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_mode_mtu(16, IfaceRole::Unicast, InterfaceMode::Full, 20)
            .tx_channel;
        let iface = mgr.ifaces[0].address;
        let packet = packet_with_wire_len(20);

        let trace =
            mgr.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet: packet.clone() }).await;

        assert_eq!(trace.matched_ifaces, 1);
        assert_eq!(trace.sent_ifaces, 1);
        assert_eq!(trace.failed_ifaces, 0);
        let sent = rx.try_recv().expect("packet should fit configured mtu");
        assert_eq!(sent.packet, packet);
    }

    #[tokio::test]
    async fn broadcast_over_one_iface_mtu_still_uses_larger_iface() {
        let mut mgr = InterfaceManager::new(16);
        let mut small_rx = mgr
            .new_channel_with_role_mode_mtu(16, IfaceRole::Unicast, InterfaceMode::Full, 20)
            .tx_channel;
        let mut large_rx = mgr
            .new_channel_with_role_mode_mtu(16, IfaceRole::Unicast, InterfaceMode::Full, 24)
            .tx_channel;
        let packet = packet_with_wire_len(22);

        let trace =
            mgr.send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() }).await;

        assert_eq!(trace.matched_ifaces, 2);
        assert_eq!(trace.sent_ifaces, 1);
        assert_eq!(trace.failed_ifaces, 1);
        assert!(small_rx.try_recv().is_err());
        let sent = large_rx.try_recv().expect("larger iface should receive packet");
        assert_eq!(sent.packet, packet);
    }

    #[test]
    fn set_announce_pacing_updates_registered_iface() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert!(mgr.set_announce_pacing(*channel.address(), 1200, 5));
        assert_eq!(mgr.announce_pacing(channel.address()), Some((1200, 5)));
    }

    #[test]
    fn set_shared_config_updates_registered_iface_and_virtual_copy() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        let config = InterfaceSharedConfig {
            bootstrap_only: Some(true),
            ifac_size: Some(16),
            network_name: Some("field-net".to_string()),
            passphrase: Some("shared-secret".to_string()),
            ingress_control: Some(false),
            discoverable: Some(true),
            discovery_name: Some("field node".to_string()),
            latitude: Some(45.5),
            ..InterfaceSharedConfig::default()
        };

        assert!(mgr.set_shared_config(*channel.address(), config.clone()));
        assert_eq!(mgr.shared_config(channel.address()), Some(&config));

        let virtual_iface = mgr
            .register_virtual_iface(*channel.address(), IfaceRole::Unicast)
            .expect("virtual iface");
        assert_eq!(mgr.shared_config(&virtual_iface), Some(&config));
    }

    #[test]
    fn set_mode_updates_registered_iface() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert!(mgr.set_mode(*channel.address(), InterfaceMode::Roaming));
        assert_eq!(mgr.mode(channel.address()), Some(InterfaceMode::Roaming));
    }

    #[test]
    fn full_hash_roundtrips_to_internal_address() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        let full_hash = mgr.full_hash(channel.address()).expect("full hash");
        assert_eq!(full_hash.as_slice().len(), crate::hash::HASH_SIZE);
        assert_eq!(mgr.address_for_full_hash(&full_hash), Some(*channel.address()));
    }

    #[test]
    fn role_returns_none_for_unknown_address() {
        let mgr = InterfaceManager::new(16);
        let fake = AddressHash::new_from_hash(&Hash::new_from_slice(&[0u8; 32]));
        assert_eq!(mgr.role(&fake), None);
        assert_eq!(mgr.mode(&fake), None);
    }

    #[test]
    fn each_new_channel_gets_a_unique_address_hash() {
        let mut mgr = InterfaceManager::new(16);
        let a = *mgr.new_channel(16).address();
        let b = *mgr.new_channel(16).address();
        let c = *mgr.new_channel_with_role(16, IfaceRole::Multicast).address();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn stop_interface_marks_iface_stopped_and_cleanup_removes_it() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel_with_role(16, IfaceRole::Multicast);
        let addr = *channel.address();
        assert_eq!(mgr.iface_count(), 1);
        assert!(mgr.stop_interface(addr));
        assert_eq!(mgr.iface_count(), 0);
    }

    fn packet_with_wire_len(wire_len: usize) -> Packet {
        let mut packet = Packet::default();
        let base_len = packet.to_bytes().expect("default packet serializes").len();
        assert!(wire_len >= base_len, "test packet wire length must include header");
        packet.data.safe_write(&vec![0; wire_len - base_len]);
        assert_eq!(packet.to_bytes().expect("sized packet serializes").len(), wire_len);
        packet
    }

    #[test]
    fn iface_source_default_is_none() {
        let src = IfaceSource::default();
        assert_eq!(src, IfaceSource::None);
    }

    #[test]
    fn iface_role_default_is_unicast() {
        let role = IfaceRole::default();
        assert_eq!(role, IfaceRole::Unicast);
    }

    #[test]
    fn interface_mode_parse_matches_python_aliases() {
        assert_eq!(InterfaceMode::parse("full"), Ok(Some(InterfaceMode::Full)));
        assert_eq!(InterfaceMode::parse("accesspoint"), Ok(Some(InterfaceMode::AccessPoint)));
        assert_eq!(InterfaceMode::parse("ap"), Ok(Some(InterfaceMode::AccessPoint)));
        assert_eq!(InterfaceMode::parse("pointtopoint"), Ok(Some(InterfaceMode::PointToPoint)));
        assert_eq!(InterfaceMode::parse("ptp"), Ok(Some(InterfaceMode::PointToPoint)));
        assert_eq!(InterfaceMode::parse("roaming"), Ok(Some(InterfaceMode::Roaming)));
        assert_eq!(InterfaceMode::parse("boundary"), Ok(Some(InterfaceMode::Boundary)));
        assert_eq!(InterfaceMode::parse("gw"), Ok(Some(InterfaceMode::Gateway)));
        assert!(InterfaceMode::parse("unknown").is_err());
    }

    #[test]
    fn interface_mode_discovers_unknown_paths_matches_python_constant() {
        assert!(!InterfaceMode::Full.discovers_unknown_paths());
        assert!(InterfaceMode::AccessPoint.discovers_unknown_paths());
        assert!(!InterfaceMode::PointToPoint.discovers_unknown_paths());
        assert!(InterfaceMode::Roaming.discovers_unknown_paths());
        assert!(!InterfaceMode::Boundary.discovers_unknown_paths());
        assert!(InterfaceMode::Gateway.discovers_unknown_paths());
    }

    #[test]
    fn virtual_iface_inherits_host_mode() {
        let mut mgr = InterfaceManager::new(16);
        let host = *mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Multicast, InterfaceMode::Gateway)
            .address();
        let virtual_iface =
            mgr.register_virtual_iface(host, IfaceRole::VirtualUnicast).expect("virtual iface");
        assert_eq!(mgr.mode(&virtual_iface), Some(InterfaceMode::Gateway));
    }

    #[test]
    fn virtual_iface_inherits_host_announce_pacing() {
        let mut mgr = InterfaceManager::new(16);
        let host = *mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Multicast, InterfaceMode::Gateway)
            .address();
        assert!(mgr.set_announce_pacing(host, 1200, 5));
        let virtual_iface =
            mgr.register_virtual_iface(host, IfaceRole::VirtualUnicast).expect("virtual iface");
        assert_eq!(mgr.announce_pacing(&virtual_iface), Some((1200, 5)));
    }

    #[test]
    fn accepted_child_can_inherit_parent_runtime_config() {
        let mut mgr = InterfaceManager::new(16);
        let parent = *mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Gateway)
            .address();
        let child = *mgr.new_channel(16).address();
        let shared_config = InterfaceSharedConfig {
            announce_rate_target: Some(120),
            announce_rate_grace: Some(2),
            announce_rate_penalty: Some(30),
            ingress_control: Some(false),
            egress_control: Some(true),
            ..Default::default()
        };

        assert!(mgr.set_outgoing(parent, false));
        assert!(mgr.set_announce_pacing(parent, 1200, 5));
        assert!(mgr.set_shared_config(parent, shared_config.clone()));

        assert!(mgr.inherit_runtime_config(parent, child));

        assert_eq!(mgr.mode(&child), Some(InterfaceMode::Gateway));
        assert_eq!(mgr.outgoing(&child), Some(false));
        assert_eq!(mgr.announce_pacing(&child), Some((1200, 5)));
        assert_eq!(mgr.shared_config(&child), Some(&shared_config));
    }

    #[test]
    fn virtual_iface_inherits_host_mtu() {
        let mut mgr = InterfaceManager::new(16);
        let host = *mgr.new_channel_with_role_mode_mtu(
            16,
            IfaceRole::Multicast,
            InterfaceMode::Gateway,
            220,
        )
        .address();

        let virtual_iface =
            mgr.register_virtual_iface(host, IfaceRole::VirtualUnicast).expect("virtual iface");

        assert_eq!(mgr.mtu(&virtual_iface), Some(220));
    }

    #[tokio::test]
    async fn access_point_blocks_remote_announce_broadcasts() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::AccessPoint)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Full),
                }),
            )
            .await;
        assert_eq!(trace.sent_ifaces, 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn outgoing_disabled_iface_drops_broadcast_tx() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(16).tx_channel;
        let iface = mgr.ifaces[0].address;
        assert!(mgr.set_outgoing(iface, false));
        let packet = Packet::default();

        let trace =
            mgr.send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet }).await;

        assert_eq!(trace.sent_ifaces, 0);
        assert_eq!(trace.matched_ifaces, 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn outgoing_disabled_iface_drops_direct_tx() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(16).tx_channel;
        let iface = mgr.ifaces[0].address;
        assert!(mgr.set_outgoing(iface, false));
        let packet = Packet::default();

        let trace = mgr.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet }).await;

        assert_eq!(trace.sent_ifaces, 0);
        assert_eq!(trace.matched_ifaces, 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn scoped_broadcast_targets_only_selected_iface_but_keeps_broadcast_tx_type() {
        let mut mgr = InterfaceManager::new(16);
        let mut first_rx = mgr.new_channel(16).tx_channel;
        let mut second_rx = mgr.new_channel(16).tx_channel;
        let first_iface = mgr.ifaces[0].address;
        let second_iface = mgr.ifaces[1].address;

        let trace = mgr
            .send_broadcast_on_iface(second_iface, path_request_packet())
            .await;

        assert_eq!(trace.matched_ifaces, 1);
        assert_eq!(trace.sent_ifaces, 1);
        assert!(first_rx.try_recv().is_err());
        let sent = second_rx.try_recv().expect("selected iface should receive scoped broadcast");
        assert_eq!(sent.tx_type, TxMessageType::Broadcast(None));

        let second_trace = mgr
            .send_broadcast_on_iface(first_iface, path_request_packet())
            .await;
        assert_eq!(second_trace.matched_ifaces, 1);
        assert_eq!(second_trace.sent_ifaces, 1);
        assert!(first_rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn egress_control_limits_rapid_path_request_broadcasts() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(16).tx_channel;
        let iface = mgr.ifaces[0].address;
        assert!(mgr.set_shared_config(
            iface,
            InterfaceSharedConfig {
                egress_control: Some(true),
                ec_pr_freq: Some(0.1),
                ..Default::default()
            },
        ));

        for _ in 0..OUTGOING_PR_MIN_LIMIT_SAMPLES {
            let trace = mgr
                .send_recursive_path_request(TxMessage {
                    tx_type: TxMessageType::Broadcast(None),
                    packet: path_request_packet(),
                })
                .await;
            assert_eq!(trace.sent_ifaces, 1);
            assert!(rx.try_recv().is_ok());
        }

        let limited = mgr
            .send_recursive_path_request(TxMessage {
                tx_type: TxMessageType::Broadcast(None),
                packet: path_request_packet(),
            })
            .await;
        assert_eq!(limited.matched_ifaces, 1);
        assert_eq!(limited.sent_ifaces, 0);
        assert_eq!(limited.failed_ifaces, 1);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn egress_control_false_does_not_limit_path_request_broadcasts() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(16).tx_channel;
        let iface = mgr.ifaces[0].address;
        assert!(mgr.set_shared_config(
            iface,
            InterfaceSharedConfig {
                egress_control: Some(false),
                ec_pr_freq: Some(0.1),
                ..Default::default()
            },
        ));

        for _ in 0..=OUTGOING_PR_MIN_LIMIT_SAMPLES {
            let trace = mgr
                .send_recursive_path_request(TxMessage {
                    tx_type: TxMessageType::Broadcast(None),
                    packet: path_request_packet(),
                })
                .await;
            assert_eq!(trace.sent_ifaces, 1);
            assert!(rx.try_recv().is_ok());
        }
    }

    #[tokio::test]
    async fn egress_control_does_not_limit_regular_path_request_broadcasts() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(16).tx_channel;
        let iface = mgr.ifaces[0].address;
        assert!(mgr.set_shared_config(
            iface,
            InterfaceSharedConfig {
                egress_control: Some(true),
                ec_pr_freq: Some(0.1),
                ..Default::default()
            },
        ));

        for _ in 0..=OUTGOING_PR_MIN_LIMIT_SAMPLES {
            let trace = mgr
                .send(TxMessage {
                    tx_type: TxMessageType::Broadcast(None),
                    packet: path_request_packet(),
                })
                .await;
            assert_eq!(trace.sent_ifaces, 1);
            assert!(rx.try_recv().is_ok());
        }
    }

    #[tokio::test]
    async fn recursive_path_request_waits_for_queued_announces_like_python() {
        let mut mgr = InterfaceManager::new(16);
        let mut source_rx = mgr.new_channel(16).tx_channel;
        let mut target_rx = mgr.new_channel(16).tx_channel;
        let source_iface = mgr.ifaces[0].address;
        let queued_announce = announce_packet_with(1, b"queued-paced-announce", 1);
        mgr.ifaces[0].announce_queue.push_back(QueuedAnnounce {
            message: TxMessage {
                tx_type: TxMessageType::Broadcast(None),
                packet: queued_announce,
            },
            queued_at: Instant::now(),
            emitted: 0,
        });

        let blocked = mgr
            .send_recursive_path_request(TxMessage {
                tx_type: TxMessageType::Broadcast(Some(source_iface)),
                packet: path_request_packet_with_data(),
            })
            .await;
        assert_eq!(blocked.matched_ifaces, 1);
        assert_eq!(blocked.sent_ifaces, 0);
        assert_eq!(blocked.failed_ifaces, 1);
        assert!(source_rx.try_recv().is_err());
        assert!(target_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn recursive_path_request_waits_for_active_announce_cap_like_python() {
        let mut mgr = InterfaceManager::new(16);
        let mut source_rx = mgr.new_channel(16).tx_channel;
        let mut target_rx = mgr.new_channel(16).tx_channel;
        let source_iface = mgr.ifaces[0].address;
        mgr.ifaces[0].announce_allowed_at = Instant::now() + Duration::from_secs(30);

        let blocked = mgr
            .send_recursive_path_request(TxMessage {
                tx_type: TxMessageType::Broadcast(Some(source_iface)),
                packet: path_request_packet_with_data(),
            })
            .await;
        assert_eq!(blocked.matched_ifaces, 1);
        assert_eq!(blocked.sent_ifaces, 0);
        assert_eq!(blocked.failed_ifaces, 1);
        assert!(source_rx.try_recv().is_err());
        assert!(target_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn recursive_path_request_advances_announce_cap_like_python() {
        let mut mgr = InterfaceManager::new(16);
        let mut source_rx = mgr.new_channel(16).tx_channel;
        let mut target_rx = mgr.new_channel(16).tx_channel;
        let source_iface = mgr.ifaces[0].address;
        mgr.ifaces[0].announce_bitrate_bps = 1;
        mgr.ifaces[0].announce_cap_percent = 1;

        let before = Instant::now();
        let packet = path_request_packet_with_data();
        let sent = mgr
            .send_recursive_path_request(TxMessage {
                tx_type: TxMessageType::Broadcast(Some(source_iface)),
                packet: packet.clone(),
            })
            .await;
        assert_eq!(sent.matched_ifaces, 1);
        assert_eq!(sent.sent_ifaces, 1);
        assert!(source_rx.try_recv().is_err());
        assert_eq!(target_rx.try_recv().expect("recursive request sent").packet, packet);
        assert!(
            mgr.ifaces[0].announce_allowed_at > before,
            "recursive path requests should advance the source iface announce cap window"
        );
    }

    #[tokio::test]
    async fn recursive_path_request_uses_source_iface_pacing_like_python() {
        let mut mgr = InterfaceManager::new(16);
        let mut source_rx = mgr.new_channel(16).tx_channel;
        let mut target_rx = mgr.new_channel(16).tx_channel;
        let source_iface = mgr.ifaces[0].address;
        mgr.ifaces[1].announce_allowed_at = Instant::now() + Duration::from_secs(30);

        let packet = path_request_packet_with_data();
        let sent = mgr
            .send_recursive_path_request(TxMessage {
                tx_type: TxMessageType::Broadcast(Some(source_iface)),
                packet: packet.clone(),
            })
            .await;

        assert_eq!(sent.matched_ifaces, 1);
        assert_eq!(sent.sent_ifaces, 1);
        assert_eq!(sent.failed_ifaces, 0);
        assert!(source_rx.try_recv().is_err());
        assert_eq!(target_rx.try_recv().expect("recursive request sent").packet, packet);
    }

    #[tokio::test]
    async fn saturated_broadcast_queue_returns_without_enqueue_timeout() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(1).tx_channel;
        let iface = mgr.ifaces[0].address;
        let first = Packet::default();
        let second = Packet::default();

        let fill =
            mgr.send(TxMessage { tx_type: TxMessageType::Direct(iface), packet: first }).await;
        assert_eq!(fill.sent_ifaces, 1);

        let started = Instant::now();
        let trace = mgr
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: second })
            .await;

        assert!(
            started.elapsed() < Duration::from_millis(50),
            "broadcast dispatch must not wait for the enqueue timeout on saturated queues"
        );
        assert_eq!(trace.matched_ifaces, 1);
        assert_eq!(trace.sent_ifaces, 0);
        assert_eq!(trace.failed_ifaces, 1);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn roaming_blocks_remote_announce_without_allowed_next_hop() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Roaming)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Boundary),
                }),
            )
            .await;
        assert_eq!(trace.sent_ifaces, 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn roaming_allows_local_announce_broadcasts() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Roaming)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: true,
                    next_hop_iface_mode: None,
                }),
            )
            .await;
        assert_eq!(trace.sent_ifaces, 1);
        assert_eq!(rx.try_recv().expect("announce").packet, packet);
    }

    #[tokio::test]
    async fn boundary_blocks_remote_announce_from_roaming_next_hop() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Boundary)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Roaming),
                }),
            )
            .await;
        assert_eq!(trace.sent_ifaces, 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn boundary_allows_remote_announce_from_boundary_next_hop() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Boundary)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Boundary),
                }),
            )
            .await;
        assert_eq!(trace.sent_ifaces, 1);
        assert_eq!(rx.try_recv().expect("announce").packet, packet);
    }

    #[tokio::test]
    async fn remote_announces_are_queued_and_released_by_hop_priority() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr.new_channel(16).tx_channel;
        mgr.ifaces[0].announce_bitrate_bps = 1;

        let first = announce_packet_with(1, b"first-destination", 1);
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: first.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Full),
                }),
            )
            .await;
        assert_eq!(trace.sent_ifaces, 1);
        assert_eq!(rx.try_recv().expect("first announce").packet, first);

        let farther = announce_packet_with(4, b"farther-destination", 2);
        let nearer = announce_packet_with(2, b"nearer-destination", 3);
        let farther_trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: farther },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Full),
                }),
            )
            .await;
        let nearer_trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: nearer.clone() },
                Some(AnnounceBroadcastPolicy {
                    local_destination: false,
                    next_hop_iface_mode: Some(InterfaceMode::Full),
                }),
            )
            .await;
        assert_eq!(farther_trace.queued_ifaces, 1);
        assert_eq!(nearer_trace.queued_ifaces, 1);
        assert!(rx.try_recv().is_err());

        mgr.ifaces[0].announce_allowed_at = Instant::now();
        let release = mgr.release_queued_announces().await;
        assert_eq!(release.sent_ifaces, 1);
        assert_eq!(rx.try_recv().expect("released announce").packet, nearer);
    }

    fn announce_packet() -> Packet {
        Packet {
            header: crate::packet::Header {
                packet_type: PacketType::Announce,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn path_request_packet() -> Packet {
        Packet::default()
    }

    fn path_request_packet_with_data() -> Packet {
        Packet {
            data: crate::packet::PacketDataBuffer::new_from_slice(b"path-request"),
            ..Packet::default()
        }
    }

    fn announce_packet_with(hops: u8, seed: &[u8], emitted: u64) -> Packet {
        let mut data = [0u8; 96];
        data[..seed.len()].copy_from_slice(seed);
        let offset = crate::identity::PUBLIC_KEY_LENGTH * 2 + crate::destination::NAME_HASH_LENGTH;
        let emitted_be = emitted.to_be_bytes();
        data[offset + 5..offset + crate::destination::RAND_HASH_LENGTH]
            .copy_from_slice(&emitted_be[3..]);

        Packet {
            header: crate::packet::Header { packet_type: PacketType::Announce, hops, ..Default::default() },
            destination: AddressHash::new_from_hash(&Hash::new_from_slice(seed)),
            data: crate::packet::PacketDataBuffer::new_from_slice(&data),
            ..Default::default()
        }
    }
}
