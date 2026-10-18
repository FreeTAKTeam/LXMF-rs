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
        self.send_prepared_single_packet_guarded_with_trace(packet, identity, name, |hash| {
            observe_packet_hash(hash);
            true
        })
        .await
        .map(|trace| trace.expect("unconditional prepared handoff always accepts"))
    }

    /// Validate a prepared SINGLE packet, then decide its handoff under the
    /// transport/cache/interface locks. Returning false yields `None` without
    /// proof tracking or dispatch. The synchronous callback may register app
    /// correlation when accepting; it must not re-enter transport or invoke
    /// user callbacks. Acceptance is the handoff linearization point.
    pub async fn send_prepared_single_packet_guarded_with_trace(
        &self,
        packet: Packet,
        identity: Identity,
        name: DestinationName,
        accept_packet_hash: impl FnOnce(Hash) -> bool,
    ) -> Result<Option<SendPacketTrace>, RnsError> {
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
        Ok(handler.send_prepared_broadcast(packet, accept_packet_hash).await)
    }
}

impl TransportHandler {
    async fn send_prepared_broadcast(
        &self,
        packet: Packet,
        accept_packet_hash: impl FnOnce(Hash) -> bool,
    ) -> Option<SendPacketTrace> {
        let packet_hash = packet.hash();
        // Lock order is handler -> packet cache -> interface manager. Broadcast
        // dispatch uses try_send (including overload) and cannot suspend after
        // acceptance. Do not extend this guarantee to blocking Direct dispatch.
        let mut cache = self.packet_cache.lock().await;
        let mut interfaces = self.iface_manager.lock().await;
        if !accept_packet_hash(packet_hash) {
            return None;
        }
        self.register_outbound_packet(&mut cache, &packet);
        drop(cache);
        let dispatch = interfaces
            .send_with_announce_policy(
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet },
                None,
            )
            .await;
        let outcome = if dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0 {
            SendPacketOutcome::SentBroadcast
        } else {
            SendPacketOutcome::DroppedNoRoute
        };
        Some(SendPacketTrace::with_packet_hash(outcome, packet_hash, None, true, dispatch))
    }
}
