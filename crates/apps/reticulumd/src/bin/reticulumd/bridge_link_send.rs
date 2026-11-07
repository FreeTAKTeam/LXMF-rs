use super::*;

impl DeliveryTask {
    pub(super) async fn identify_link_for_backchannel(
        &self,
        link: &Arc<tokio::sync::Mutex<Link>>,
    ) -> Result<(), std::io::Error> {
        let link_id = *link.lock().await.id();
        let identify_payload =
            remote_control_link::build_link_identify_payload(&self.signer, &link_id);
        remote_control_link::send_link_context_packet(
            self.transport.as_ref(),
            link,
            PacketContext::LinkIdentify,
            identify_payload.as_slice(),
        )
        .await?;
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "link",
            &format!("identified local delivery destination on link={link_id}"),
        );
        Ok(())
    }
}
