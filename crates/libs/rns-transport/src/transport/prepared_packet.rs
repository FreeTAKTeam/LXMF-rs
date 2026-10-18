//! Prepared ciphertext dispatch and its destination-bound proof registration.
use super::*;

impl Transport {
    /// Broadcast an already encrypted SINGLE packet using a cached recipient.
    ///
    /// Recomputes the destination binding and registers its identity without
    /// inventing a route. The observer sees the final packet hash before handoff;
    /// it must return promptly and must not re-enter this transport. No observer
    /// or network send occurs if validation fails. Ciphertext is not re-encrypted.
    pub async fn send_prepared_single_packet_observed_with_trace(
        &self,
        packet: Packet,
        identity: Identity,
        name: DestinationName,
        observe_packet_hash: impl FnOnce(Hash),
    ) -> Result<SendPacketTrace, RnsError> {
        // Identity fields are public, so a cached address is not itself proof of
        // the key binding. Validate it before deriving the destination address.
        if Identity::new(identity.public_key, identity.verifying_key).address_hash
            != identity.address_hash
        {
            return Err(RnsError::IncorrectHash);
        }
        let destination = SingleOutputDestination::new(identity, name);
        if packet.header.packet_type != PacketType::Data
            || packet.header.destination_type != DestinationType::Single
            || packet.context != PacketContext::None
            || packet.destination != destination.desc.address_hash
        {
            return Err(RnsError::InvalidArgument);
        }
        let mut handler = self.handler.lock().await;
        if let Some(existing) = handler.single_out_destinations.get(&packet.destination) {
            if super::announce::identity_drifted(&existing.lock().await.identity, &identity) {
                return Err(RnsError::IncorrectHash);
            }
        } else {
            handler
                .single_out_destinations
                .insert(packet.destination, Arc::new(Mutex::new(destination)));
        }
        Ok(handler.send_prepared_broadcast(packet, observe_packet_hash).await)
    }
}

impl TransportHandler {
    async fn send_prepared_broadcast(
        &self,
        packet: Packet,
        observe_packet_hash: impl FnOnce(Hash),
    ) -> SendPacketTrace {
        let packet_hash = packet.hash();
        observe_packet_hash(packet_hash);
        // The ordinary send helper installs proof context before interface I/O.
        // Calling prepare_outbound_packet here would encrypt the payload twice.
        let dispatch =
            self.send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet }).await;
        let outcome = if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
            SendPacketOutcome::SentBroadcast
        } else {
            SendPacketOutcome::DroppedNoRoute
        };
        SendPacketTrace::with_packet_hash(outcome, packet_hash, None, true, dispatch)
    }
}
