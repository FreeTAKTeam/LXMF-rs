async fn handle_unknown_owner_local_link_request<'a>(
    packet: &Packet,
    ingress_iface: AddressHash,
    handler: &mut MutexGuard<'a, TransportHandler>,
) -> bool {
    if packet.header.packet_type != PacketType::LinkRequest
        || !handler.config.transport_enabled
        || !handler.config.connected_to_shared_instance
    {
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
            return false;
        };
        // Fail closed instead of flooding when no route identifies one owner.
        if shared_ifaces.next().is_some() {
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
    handler.link_table.add(
        &forwarded,
        packet.destination,
        ingress_iface,
        packet.destination,
        shared_iface,
    );
    handler
        .send(TxMessage { tx_type: TxMessageType::Direct(shared_iface), packet: forwarded })
        .await;
    true
}
