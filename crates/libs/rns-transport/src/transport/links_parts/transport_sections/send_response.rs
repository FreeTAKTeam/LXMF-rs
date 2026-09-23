impl Transport {
    /// Send a Link request response using Reticulum's packet/Resource rule.
    ///
    /// For ordinary responses, `data` must be the packed
    /// `[request_id, response]` envelope. Envelopes whose encoded length is
    /// at most the negotiated Link MDU use one `Response` packet; larger
    /// envelopes use a response Resource. A metadata-bearing file response
    /// always uses a Resource, matching `RNS.Link.handle_request`.
    ///
    /// Returns `None` when a packet was sent and `Some(resource_hash)` when a
    /// Resource was advertised. For metadata-bearing file responses, `data`
    /// is the raw file content; otherwise it is the packed response envelope.
    pub async fn send_response(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Option<Hash>, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let use_resource = {
            let guard = link.lock().await;
            metadata.is_some() || data.len() > guard.link_mdu()
        };

        if use_resource {
            return self
                .send_response_resource(link_id, request_id, data, metadata)
                .await
                .map(Some);
        }

        let packet = link.lock().await.response_packet(&data)?;
        match self.send_link_packet_on_bound_iface(&link, packet).await {
            SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast => Ok(None),
            SendPacketOutcome::DroppedMissingDestinationIdentity
            | SendPacketOutcome::DroppedCiphertextTooLarge
            | SendPacketOutcome::DroppedEncryptFailed
            | SendPacketOutcome::DroppedNoRoute => Err(RnsError::ConnectionError),
        }
    }
}
