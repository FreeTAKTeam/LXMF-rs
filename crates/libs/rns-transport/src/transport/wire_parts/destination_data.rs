use super::*;

pub(super) async fn forward_destination_data(
    packet: &Packet,
    ingress: AddressHash,
    handler: &TransportHandler,
) -> bool {
    let decision = super::super::path::route_inbound_packet(&handler.path_table, packet, None);
    let Some(outbound) = decision.next_iface else {
        return false;
    };
    let allow = handler.config.transport_enabled || {
        let manager = handler.iface_manager.lock().await;
        manager.is_local_client_interface(&ingress)
            || (handler.path_table.get(&packet.destination).is_some_and(|entry| entry.hops == 0)
                && manager.is_local_client_interface(&outbound))
    };
    if !allow {
        return false;
    }
    let dispatch = handler
        .send(TxMessage { tx_type: TxMessageType::Direct(outbound), packet: decision.packet })
        .await;
    let accepted = dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0;
    if accepted {
        handler.packet_cache.lock().await.note_forwarded_data(packet.hash(), outbound);
    }
    accepted
}
