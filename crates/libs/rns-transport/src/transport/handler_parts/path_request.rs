impl TransportHandler {
    pub(super) async fn send_recursive_path_request_with_modes(
        &self,
        message: TxMessage,
        allowed_modes: Option<&[InterfaceMode]>,
    ) -> TxDispatchTrace {
        let packet = message.packet.clone();
        {
            let mut cache = self.packet_cache.lock().await;
            self.register_outbound_packet(&mut cache, &packet);
        }
        let dispatch = self
            .iface_manager
            .lock()
            .await
            .send_recursive_path_request_with_modes(message, allowed_modes)
            .await;
        if dispatch.sent_ifaces > 0 {
            self.note_link_packet_sent(&packet).await;
        }
        dispatch
    }
}
