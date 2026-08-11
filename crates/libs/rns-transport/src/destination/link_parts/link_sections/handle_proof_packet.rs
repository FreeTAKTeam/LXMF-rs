impl Link {
    /// Handle an inbound `Proof` packet: either a per-packet delivery proof on
    /// an active link, or the `LinkRequestProof` that activates a pending one.
    fn handle_proof_packet(&mut self, packet: &Packet, iface: AddressHash) -> LinkHandleResult {
        if self.status == LinkStatus::Active
            && matches!(packet.context, PacketContext::None | PacketContext::LinkProof)
        {
            if let Ok(hash) = self.validate_packet_proof(packet) {
                self.note_inbound(packet.context);
                if let Some(pending) = self.channel_pending.remove(&hash) {
                    self.channel_states.insert(pending.sequence, ChannelMessageState::Delivered);
                    self.note_channel_delivery();
                }
                return LinkHandleResult::None;
            }
        }
        if self.status != LinkStatus::Pending || packet.context != PacketContext::LinkRequestProof {
            return LinkHandleResult::None;
        }
        let Ok(identity) = validate_link_request_proof_packet(&self.destination, &self.id, packet)
        else {
            log::warn!("link({}): proof is not valid", self.id);
            return LinkHandleResult::None;
        };
        log::debug!("link({}): has been proved", self.id);

        // The proof carries the responder's own MTU signalling, and until now
        // it was read only to verify the signature over it and then dropped —
        // so an initiator never learned what the far end could actually carry,
        // and every size it derived came from its own interface instead.
        // Capture it (clamped on the way in, like an inbound request's) so
        // `link_mtu()` reports the negotiated value.
        const MTU_PROOF_LEN: usize = SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH + LINK_MTU_SIZE;
        if packet.data.len() >= MTU_PROOF_LEN {
            let start = SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH;
            let mut bytes = [0u8; LINK_MTU_SIZE];
            bytes.copy_from_slice(&packet.data.as_slice()[start..start + LINK_MTU_SIZE]);
            self.signalling = Some(clamp_link_signalling(bytes));
        }

        self.handshake(identity);
        self.ingress_iface.get_or_insert(iface);

        self.status = LinkStatus::Active;
        self.rtt = self.request_time.elapsed();
        self.activated_at = Some(Instant::now());
        self.last_proof = self.activated_at;
        self.stale_since = None;
        self.update_keepalive_timing();
        self.refresh_channel_flow_control();

        log::debug!("link({}): activated", self.id);

        self.post_event(LinkEvent::Activated);

        LinkHandleResult::Activated
    }
}
