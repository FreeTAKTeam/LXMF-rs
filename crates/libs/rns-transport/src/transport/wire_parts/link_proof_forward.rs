async fn forward_link_request_proof(
    packet: &Packet,
    handler: &mut TransportHandler,
    iface: AddressHash,
) -> Option<(Packet, AddressHash)> {
    if packet.context != PacketContext::LinkRequestProof
        || packet.header.packet_type != PacketType::Proof
        || packet.header.destination_type != DestinationType::Link
        || !handler.can_relay_link(&packet.destination, iface).await
    {
        return None;
    }

    let Some((original_destination, expected_iface)) =
        handler.link_table.proof_validation_context(&packet.destination)
    else {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=no_link_table_entry link={} iface={}",
            handler.config.name,
            packet.destination,
            iface
        );
        return None;
    };
    if expected_iface != iface {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=wrong_iface link={} expected={} got={}",
            handler.config.name,
            packet.destination,
            expected_iface,
            iface
        );
        return None;
    }

    let authenticated = if let Some(destination) =
        handler.single_out_destinations.get(&original_destination).cloned()
    {
        let destination = destination.lock().await;
        if crate::destination::link::validate_link_request_proof_packet(
            &destination.desc,
            &packet.destination,
            packet,
        )
        .is_err()
        {
            log::debug!("[tp-diag] lrproof_forward_skip node={} reason=invalid_signature_or_shape link={} dst={} iface={}",
                handler.config.name, packet.destination, original_destination, iface);
            return None;
        }
        true
    } else {
        if handler.link_table.allows_shared_owner_proof_on_iface(&packet.destination, iface) {
            log::debug!(
                "[tp-diag] lrproof_forward_trust_shared_owner node={} link={} dst={} iface={}",
                handler.config.name,
                packet.destination,
                original_destination,
                iface
            );
            false
        } else {
            log::debug!("[tp-diag] lrproof_forward_skip node={} reason=missing_destination_identity link={} dst={}",
                handler.config.name, packet.destination, original_destination);
            return None;
        }
    };
    let previous_hops = handler.link_table.remaining_hops(&packet.destination);
    let forwarded = handler.link_table.handle_proof(packet)?;
    if authenticated && previous_hops != Some(packet.header.hops) {
        handler.path_table.rebalance_hops(&original_destination, packet.header.hops);
    }
    Some(forwarded)
}
