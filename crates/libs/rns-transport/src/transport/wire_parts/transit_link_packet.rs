/// One current table decision handles Data, ordinary proofs, Resource traffic
/// and KeepAlive before local consumers or source/proof-context bookkeeping.
async fn forward_transit_link_packet(
    packet: &Packet,
    handler: &mut TransportHandler,
    iface: AddressHash,
) -> bool {
    if packet.header.destination_type != DestinationType::Link
        || packet.context == PacketContext::LinkRequestProof
        || !handler.link_table.knows(&packet.destination)
        || !handler.can_relay_link(&packet.destination, iface).await
    {
        return false;
    }
    if let Some((forwarded, target)) = handler.link_table.handle_link_packet(packet, iface) {
        if packet.header.packet_type == PacketType::Data {
            handler.packet_cache.lock().await.note_source(packet, iface);
        }
        handler.send(TxMessage { tx_type: TxMessageType::Direct(target), packet: forwarded }).await;
    } else {
        log::debug!("dropping transit Link packet before validation or with wrong interface/hops link={} iface={} hops={}",
            packet.destination, iface, packet.header.hops);
    }
    true
}
