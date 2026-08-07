async fn should_forward_link_table_proof(
    packet: &Packet,
    handler: &TransportHandler,
    iface: AddressHash,
) -> bool {
    if !handler.config.transport_enabled {
        log::debug!(
            "[tp-diag] link_proof_forward_skip node={} reason=transport_disabled link={} iface={}",
            handler.config.name,
            packet.destination,
            iface
        );
        return false;
    }

    if packet.context != PacketContext::LinkRequestProof {
        return true;
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
        return false;
    };
    if expected_iface != iface {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=wrong_iface link={} expected={} got={}",
            handler.config.name,
            packet.destination,
            expected_iface,
            iface
        );
        return false;
    }

    let Some(destination) = handler.single_out_destinations.get(&original_destination).cloned()
    else {
        log::debug!(
            "[tp-diag] lrproof_forward_skip node={} reason=missing_destination_identity link={} dst={}",
            handler.config.name,
            packet.destination,
            original_destination
        );
        return false;
    };
    let destination = destination.lock().await;

    let valid = crate::destination::link::validate_link_request_proof_packet(
        &destination.desc,
        &packet.destination,
        packet,
    )
    .is_ok();
    log::debug!(
        "[tp-diag] lrproof_forward_validate node={} link={} dst={} iface={} valid={}",
        handler.config.name,
        packet.destination,
        original_destination,
        iface,
        valid
    );
    valid
}
