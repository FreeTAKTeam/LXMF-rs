impl Link {
    #[cfg(test)]
    pub(crate) fn expected_proof_hops(&self) -> Option<u8> {
        self.expected_proof_hops
    }

    /// Bind only the first dispatch of this pending request generation. A public
    /// restart or an old delayed dispatch cannot rewrite another generation.
    pub(crate) fn bind_request_hops(&mut self, request_id: LinkId, hops: u8) {
        if self.is_initiator
            && self.proof_generation_ready
            && self.status == LinkStatus::Pending
            && self.id == request_id
        {
            self.expected_proof_hops.get_or_insert(hops);
        }
    }

    pub(crate) fn select_pending_proof(&mut self, packet: &Packet) -> PendingProofSelection {
        if !self.proof_generation_ready
            || !self.is_initiator
            || self.status != LinkStatus::Pending
            || packet.destination != self.id
            || packet.header.packet_type != PacketType::Proof
            || packet.header.destination_type != DestinationType::Link
            || packet.context != PacketContext::LinkRequestProof
        {
            return PendingProofSelection::Rejected;
        }
        // Standalone Link primitives have no Transport route. Native outbound
        // publication binds even an unknown route to PATHFINDER_M before send.
        let Some(expected) = self.expected_proof_hops else {
            return PendingProofSelection::Matched;
        };
        if packet.header.hops == expected {
            return PendingProofSelection::Matched;
        }
        if self.proof_hops_rebalanced
            || !link_proof_mode_matches(packet)
            || validate_link_request_proof_packet(&self.destination, &self.id, packet).is_err()
        {
            return PendingProofSelection::Rejected;
        }
        self.expected_proof_hops = Some(packet.header.hops);
        self.proof_hops_rebalanced = true;
        PendingProofSelection::Rebalanced
    }
}

fn link_proof_mode_matches(packet: &Packet) -> bool {
    let mode = packet
        .data
        .as_slice()
        .get(SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH)
        .map_or(LinkMode::DEFAULT.mode_bits(), |byte| u32::from(byte >> 5));
    mode == LinkMode::DEFAULT.mode_bits()
}

#[cfg(test)]
mod proof_hop_tests {
    use super::*;

    fn pending_pair() -> (Link, Link) {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: super::super::DestinationName::new("lxmf", "proof-hops"),
        };
        let events = tokio::sync::broadcast::channel(8).0;
        let mut local = Link::new(destination, events.clone());
        let request = local.request();
        local.bind_request_hops(*local.id(), 2);
        let peer = Link::new_from_request(&request, signer.sign_key().clone(), destination, events)
            .expect("native responder");
        (local, peer)
    }

    #[test]
    fn unsigned_legacy_suffix_cannot_rebalance_or_activate_pending_link() {
        for extra in [1, 2] {
            let (mut local, mut peer) = pending_pair();
            peer.signalling = None;
            let mut proof = peer.prove();
            assert_eq!(proof.data.len(), 96);
            for _ in 0..extra {
                proof.data.safe_write(&[0]);
            }
            proof.header.hops = 3;
            assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rejected));
            assert_eq!(local.expected_proof_hops, Some(2));
            assert!(!local.proof_hops_rebalanced);
            proof.header.hops = 2;
            assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Matched));
            assert!(matches!(
                local.handle_packet(&proof, AddressHash::new_empty()),
                LinkHandleResult::None
            ));
            assert_eq!(local.status(), LinkStatus::Pending);
        }
    }

    #[test]
    fn valid_signature_with_other_cipher_mode_cannot_rebalance_or_activate() {
        let (mut local, mut peer) = pending_pair();
        let signalling = peer.signalling.as_mut().expect("native request signalling");
        signalling[0] ^= 1 << 5;
        let mut proof = peer.prove();
        assert!(
            validate_link_request_proof_packet(&local.destination, &local.id, &proof).is_ok(),
            "negative control has a valid signature, only the negotiated mode differs"
        );
        proof.header.hops = 3;
        assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rejected));
        assert_eq!(local.expected_proof_hops, Some(2));
        proof.header.hops = 2;
        assert!(matches!(
            local.handle_packet(&proof, AddressHash::new_empty()),
            LinkHandleResult::None
        ));
        assert_eq!(local.status(), LinkStatus::Pending);
    }

    #[test]
    fn pending_retry_preserves_one_shot_rebalance_and_restart_rejects_delayed_dispatch() {
        let (mut local, mut peer) = pending_pair();
        let old_id = *local.id();
        let mut proof = peer.prove();
        proof.header.hops = 3;
        assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rebalanced));
        let retry = local.request();
        assert_eq!(LinkId::from(&retry), old_id);
        local.bind_request_hops(old_id, 6);
        assert_eq!(local.expected_proof_hops, Some(3));
        assert!(local.proof_hops_rebalanced);
        proof.header.hops = 4;
        assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rejected));
        local.close();
        let _new_request = local.request();
        assert_ne!(*local.id(), old_id);
        assert_eq!(local.expected_proof_hops, None);
        assert!(!local.proof_hops_rebalanced);
        local.bind_request_hops(old_id, 7);
        assert_eq!(local.expected_proof_hops, None, "old request cannot bind restarted generation");
        assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rejected));
    }

    #[test]
    fn public_restart_fences_old_proof_and_dispatch_until_a_new_request_exists() {
        let (mut local, mut peer) = pending_pair();
        let old_id = *local.id();
        let mut proof = peer.prove();
        proof.header.hops = 3;
        assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rebalanced));
        local.restart();
        assert_eq!(*local.id(), old_id, "public id remains available until next request");
        assert_eq!(local.expected_proof_hops, None, "explicit restart clears generation binding");
        assert!(!local.proof_hops_rebalanced);
        local.bind_request_hops(old_id, 9);
        assert_eq!(local.expected_proof_hops, None, "pre-request delayed dispatch cannot bind");
        assert!(matches!(local.select_pending_proof(&proof), PendingProofSelection::Rejected));
        assert!(matches!(
            local.handle_packet(&proof, AddressHash::new_empty()),
            LinkHandleResult::None
        ));
        assert_eq!(local.status(), LinkStatus::Pending);
        let request = local.request();
        assert_ne!(LinkId::from(&request), old_id);
        local.bind_request_hops(LinkId::from(&request), 2);
        assert_eq!(local.expected_proof_hops, Some(2));
    }
}
