async fn handle_outbound_link_proof(
    packet: &Packet,
    handler: &mut TransportHandler,
    iface: AddressHash,
) -> Vec<TxMessage> {
    use crate::destination::link::PendingProofSelection;
    let mut rtt_messages = Vec::new();
    // The current table owner wins over local pending delivery, as in Python.
    if packet.context == PacketContext::LinkRequestProof
        && handler.link_table.knows(&packet.destination)
        && handler.can_relay_link(&packet.destination, iface).await
    {
        return rtt_messages;
    }
    for link in handler.out_links.values() {
        let mut link = link.lock().await;
        if *link.id() != packet.destination {
            continue;
        }
        if packet.context == PacketContext::LinkRequestProof {
            match link.select_pending_proof(packet) {
                PendingProofSelection::Rejected => continue,
                PendingProofSelection::Matched => {}
                PendingProofSelection::Rebalanced => handler
                    .path_table
                    .rebalance_hops(&link.destination().address_hash, packet.header.hops),
            }
            // Keep selection, remembering and validation on the same Link
            // generation. Lock order is handler -> Link -> cache; no I/O or
            // external callback occurs while remembering under the cache lock.
            if !handler.config.connected_to_shared_instance {
                handler.packet_cache.lock().await.remember_hash(packet.hash());
            }
        }
        if let LinkHandleResult::Activated = link.handle_packet(packet, iface) {
            rtt_messages.push(TxMessage {
                tx_type: TxMessageType::Direct(iface),
                packet: link.create_rtt(),
            });
        }
    }
    rtt_messages
}
