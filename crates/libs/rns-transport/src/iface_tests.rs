#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::PacketType;

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
    fn set_announce_pacing_updates_registered_iface() {
        let mut mgr = InterfaceManager::new(16);
        let channel = mgr.new_channel(16);
        assert!(mgr.set_announce_pacing(*channel.address(), 1200, 5));
        assert_eq!(mgr.announce_pacing(channel.address()), Some((1200, 5)));
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
        assert_eq!(InterfaceMode::parse("full"), Some(InterfaceMode::Full));
        assert_eq!(InterfaceMode::parse("accesspoint"), Some(InterfaceMode::AccessPoint));
        assert_eq!(InterfaceMode::parse("ap"), Some(InterfaceMode::AccessPoint));
        assert_eq!(InterfaceMode::parse("pointtopoint"), Some(InterfaceMode::PointToPoint));
        assert_eq!(InterfaceMode::parse("ptp"), Some(InterfaceMode::PointToPoint));
        assert_eq!(InterfaceMode::parse("roaming"), Some(InterfaceMode::Roaming));
        assert_eq!(InterfaceMode::parse("boundary"), Some(InterfaceMode::Boundary));
        assert_eq!(InterfaceMode::parse("gw"), Some(InterfaceMode::Gateway));
        assert_eq!(InterfaceMode::parse("unknown"), None);
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

    #[tokio::test]
    async fn access_point_blocks_remote_announce_broadcasts() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::AccessPoint)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
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
    async fn roaming_blocks_remote_announce_without_allowed_next_hop() {
        let mut mgr = InterfaceManager::new(16);
        let mut rx = mgr
            .new_channel_with_role_and_mode(16, IfaceRole::Unicast, InterfaceMode::Roaming)
            .tx_channel;
        let packet = announce_packet();
        let trace = mgr
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
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
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
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
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
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
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
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
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: first },
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
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: nearer },
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
