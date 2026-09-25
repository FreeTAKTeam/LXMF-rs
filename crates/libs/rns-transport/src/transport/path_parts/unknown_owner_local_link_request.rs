async fn handle_unknown_owner_local_link_request<'a>(
    packet: &Packet,
    ingress_iface: AddressHash,
    handler: &mut MutexGuard<'a, TransportHandler>,
) -> bool {
    if packet.header.packet_type != PacketType::LinkRequest {
        return false;
    }
    if !handler.config.transport_enabled || !handler.config.connected_to_shared_instance {
        log::trace!(
            "[tp-diag] unknown_owner_link_request_ineligible node={} link={} dst={} ingress={} transport_enabled={} connected_to_shared_instance={}",
            handler.config.name,
            LinkId::from(packet),
            packet.destination,
            ingress_iface,
            handler.config.transport_enabled,
            handler.config.connected_to_shared_instance
        );
        return false;
    }

    let shared_iface = {
        let manager = handler.iface_manager.lock().await;
        let mut shared_ifaces = manager.interface_hashes().into_iter().filter(|candidate| {
            *candidate != ingress_iface
                && manager.is_shared_instance(candidate)
                && manager.outgoing(candidate) == Some(true)
        });
        let Some(shared_iface) = shared_ifaces.next() else {
            log::trace!(
                "[tp-diag] unknown_owner_link_request_no_shared_route node={} link={} dst={} ingress={}",
                handler.config.name,
                LinkId::from(packet),
                packet.destination,
                ingress_iface
            );
            return false;
        };
        // Fail closed instead of flooding when no route identifies one owner.
        if shared_ifaces.next().is_some() {
            log::trace!(
                "[tp-diag] unknown_owner_link_request_ambiguous_shared_route node={} link={} dst={} ingress={}",
                handler.config.name,
                LinkId::from(packet),
                packet.destination,
                ingress_iface
            );
            return false;
        }
        shared_iface
    };

    let mut forwarded = packet.clone();
    forwarded.header = Header {
        ifac_flag: packet.header.ifac_flag,
        header_type: HeaderType::Type1,
        context_flag: packet.header.context_flag,
        propagation_type: PropagationType::Broadcast,
        destination_type: packet.header.destination_type,
        packet_type: packet.header.packet_type,
        hops: packet.header.hops,
    };
    forwarded.ifac = None;
    forwarded.transport = None;

    clamp_forwarded_link_request_mtu(&mut forwarded, handler, ingress_iface, shared_iface).await;
    let link_id = LinkId::from(&forwarded);
    handler.link_table.add_shared_owner_handoff(
        &forwarded,
        packet.destination,
        ingress_iface,
        packet.destination,
        shared_iface,
    );
    log::trace!(
        "[tp-diag] unknown_owner_link_request_forward node={} link={} dst={} ingress={} shared_iface={} hops={}",
        handler.config.name,
        link_id,
        packet.destination,
        ingress_iface,
        shared_iface,
        forwarded.header.hops
    );
    handler
        .send(TxMessage { tx_type: TxMessageType::Direct(shared_iface), packet: forwarded })
        .await;
    log::trace!(
        "[tp-diag] unknown_owner_link_request_queued node={} link={} dst={} shared_iface={}",
        handler.config.name,
        link_id,
        packet.destination,
        shared_iface
    );
    true
}
