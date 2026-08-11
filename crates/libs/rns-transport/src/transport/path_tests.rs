#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use crate::packet::{ContextFlag, IfacFlag, PacketDataBuffer};

    fn path_table_with_route(
        destination: AddressHash,
        received_from: AddressHash,
        hops: u8,
        iface: AddressHash,
    ) -> PathTable {
        let mut table = PathTable::new();
        assert!(table.restore_tunnel_path(
            destination,
            received_from,
            hops,
            iface,
            Hash::new_from_slice(b"packet"),
            Instant::now(),
        ));
        table
    }

    fn packet_for_route(
        destination: AddressHash,
        header_type: HeaderType,
        propagation_type: PropagationType,
        packet_type: PacketType,
        hops: u8,
        transport: Option<AddressHash>,
    ) -> Packet {
        Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type,
                context_flag: ContextFlag::Unset,
                propagation_type,
                destination_type: DestinationType::Single,
                packet_type,
                hops,
            },
            ifac: None,
            destination,
            transport,
            context: crate::packet::PacketContext::None,
            data: PacketDataBuffer::new(),
        }
    }

    #[test]
    fn outbound_direct_hop_preserves_type1_and_ifac_flag() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let table = path_table_with_route(destination, destination, 1, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::Data,
            0,
            None,
        );

        let decision = route_outbound_packet(&table, &packet, false);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.ifac_flag, IfacFlag::Open);
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn outbound_direct_hop_behind_shared_instance_promotes_to_type2_transport() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let table = path_table_with_route(destination, destination, 1, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::Data,
            0,
            None,
        );

        let decision = route_outbound_packet(&table, &packet, true);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.ifac_flag, IfacFlag::Open);
        assert_eq!(decision.packet.header.header_type, HeaderType::Type2);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Transport);
        assert_eq!(decision.packet.transport, Some(destination));
    }

    #[test]
    fn outbound_multihop_promotes_to_type2_transport() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"next_hop"));
        let table = path_table_with_route(destination, next_hop, 2, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::Data,
            0,
            None,
        );

        let decision = route_outbound_packet(&table, &packet, false);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.ifac_flag, IfacFlag::Open);
        assert_eq!(decision.packet.header.header_type, HeaderType::Type2);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Transport);
        assert_eq!(decision.packet.transport, Some(next_hop));
    }

    // Confirmed via a byte-for-byte wire capture of a reference client's own
    // successful LinkRequest against a live destination with this exact
    // path-table shape (hops=1, received_from = a relay's hash, not the
    // destination's own): it sent a plain Type1 LinkRequest and it worked.
    // Reference Reticulum's own `Transport.outbound()` (confirmed by direct
    // reading) only demotes hops==1 to Type2 when the local instance is
    // itself behind a shared instance — `received_from` never factors in.
    #[test]
    fn outbound_one_hop_via_relay_still_uses_type1() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let transport_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"transport_hop"));
        let table = path_table_with_route(destination, transport_hop, 1, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::LinkRequest,
            0,
            None,
        );

        let decision = route_outbound_packet(&table, &packet, false);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Broadcast);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn outbound_zero_hop_stays_type1_even_behind_shared_instance() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let transport_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"transport_hop"));
        let table = path_table_with_route(destination, transport_hop, 0, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::LinkRequest,
            0,
            None,
        );

        // Even connected_to_shared_instance=true doesn't demote a genuinely
        // zero-hop destination — only hops==1 does, per the real rule.
        let decision = route_outbound_packet(&table, &packet, true);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn inbound_direct_hop_strips_transport_and_preserves_hops() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        // Genuinely direct (zero real hops) is `hops == 0` under the
        // corrected semantics — see `route_inbound_packet`'s own doc
        // comment. Was `1` before inbound packets were fixed to increment
        // hops on receipt.
        let table = path_table_with_route(destination, destination, 0, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type2,
            PropagationType::Transport,
            PacketType::Data,
            1,
            Some(destination),
        );

        let decision = route_inbound_packet(&table, &packet, None);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Broadcast);
        assert_eq!(decision.packet.header.hops, 1);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn inbound_one_hop_path_strips_transport_header() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let table = path_table_with_route(destination, destination, 1, iface);
        let prior_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"prior_hop"));
        let packet = packet_for_route(
            destination,
            HeaderType::Type2,
            PropagationType::Transport,
            PacketType::Data,
            1,
            Some(prior_hop),
        );

        let decision = route_inbound_packet(&table, &packet, None);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Broadcast);
        assert_eq!(decision.packet.header.hops, 1);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn inbound_shared_instance_distance_strips_transport_at_direct_identity() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let table = path_table_with_route(destination, destination, 2, iface);
        let prior_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"shared_instance"));
        let packet = packet_for_route(
            destination,
            HeaderType::Type2,
            PropagationType::Transport,
            PacketType::Data,
            1,
            Some(prior_hop),
        );

        let decision = route_inbound_packet(&table, &packet, None);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Broadcast);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn inbound_direct_hop_type1_stays_direct() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        // Direct is hops==0 now — see the sibling test above.
        let table = path_table_with_route(destination, destination, 0, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::LinkRequest,
            1,
            None,
        );

        let decision = route_inbound_packet(&table, &packet, None);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type1);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Broadcast);
        assert_eq!(decision.packet.header.hops, 1);
        assert_eq!(decision.packet.transport, None);
    }

    #[test]
    fn inbound_two_hop_transport_keeps_type2() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let transport_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"transport_hop"));
        let table = path_table_with_route(destination, transport_hop, 2, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type1,
            PropagationType::Broadcast,
            PacketType::LinkRequest,
            2,
            None,
        );

        let decision = route_inbound_packet(&table, &packet, None);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type2);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Transport);
        assert_eq!(decision.packet.header.hops, 2);
        assert_eq!(decision.packet.transport, Some(transport_hop));
    }

    #[test]
    fn inbound_multihop_preserves_transport_hops() {
        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
        let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"iface"));
        let next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"next_hop"));
        let prior_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"prior_hop"));
        let table = path_table_with_route(destination, next_hop, 2, iface);
        let packet = packet_for_route(
            destination,
            HeaderType::Type2,
            PropagationType::Transport,
            PacketType::Data,
            1,
            Some(prior_hop),
        );

        let decision = route_inbound_packet(&table, &packet, None);

        assert_eq!(decision.next_iface, Some(iface));
        assert_eq!(decision.packet.header.header_type, HeaderType::Type2);
        assert_eq!(decision.packet.header.propagation_type, PropagationType::Transport);
        assert_eq!(decision.packet.header.hops, 1);
        assert_eq!(decision.packet.transport, Some(next_hop));
    }
}
