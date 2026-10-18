impl TransportHandler {
    /// Keep destination-bound proof context before handoff even when the shared
    /// owner alone maintains duplicate membership for this attached process.
    pub(super) fn register_outbound_packet(&self, cache: &mut PacketCache, packet: &Packet) {
        cache.track_packet(packet);
        if !self.config.connected_to_shared_instance {
            cache.remember_hash(packet.hash());
        }
    }

    pub(super) async fn send(&self, message: TxMessage) -> TxDispatchTrace {
        self.send_observed(message, |_| {}).await
    }

    pub(super) async fn send_observed(
        &self,
        message: TxMessage,
        handoff: impl FnOnce(&TxDispatchTrace),
    ) -> TxDispatchTrace {
        let packet = message.packet.clone();
        {
            let mut cache = self.packet_cache.lock().await;
            self.register_outbound_packet(&mut cache, &packet);
        }
        let announce_policy = if packet.header.packet_type == PacketType::Announce
            && matches!(message.tx_type, TxMessageType::Broadcast(_))
        {
            let next_hop_iface = self.path_table.next_hop_iface(&packet.destination);
            let mut mgr = self.iface_manager.lock().await;
            let local_destination = self.single_in_destinations.contains_key(&packet.destination);
            let policy =
                AnnounceBroadcastPolicy::for_next_hop(&mgr, next_hop_iface, local_destination);
            let dispatch = mgr.send_with_announce_policy(message, Some(policy)).await;
            handoff(&dispatch);
            if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
                self.note_link_packet_sent(&packet).await;
            }
            return dispatch;
        } else {
            None
        };
        let dispatch = self
            .iface_manager
            .lock()
            .await
            .send_with_announce_policy(message, announce_policy)
            .await;
        handoff(&dispatch);
        if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
            self.note_link_packet_sent(&packet).await;
        }
        dispatch
    }
}
