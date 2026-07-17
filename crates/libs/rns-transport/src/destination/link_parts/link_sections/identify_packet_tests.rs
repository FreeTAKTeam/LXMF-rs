// Kept separate from `new.rs` so the production module remains below the
// repository's 500-line module limit. See #476.
#[cfg(test)]
mod identify_packet_tests {
    use super::*;

    use crate::destination::{DestinationDesc, DestinationName};

    fn build_test_identify_payload(private: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
        let identity = private.as_identity();
        let mut payload = Vec::with_capacity(LINK_IDENTIFY_PAYLOAD_LENGTH);
        payload.extend_from_slice(identity.public_key_bytes());
        payload.extend_from_slice(identity.verifying_key_bytes());

        let mut signed = Vec::with_capacity(ADDRESS_HASH_SIZE + PUBLIC_KEY_LENGTH * 2);
        signed.extend_from_slice(link_id.as_slice());
        signed.extend_from_slice(identity.public_key_bytes());
        signed.extend_from_slice(identity.verifying_key_bytes());
        payload.extend_from_slice(&private.sign(&signed).to_bytes());
        payload
    }

    fn linked_pair() -> (
        Link,
        Link,
        AddressHash,
        tokio::sync::broadcast::Receiver<LinkEventData>,
    ) {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, mut rx) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound = Link::new_from_request(
            &request,
            signer.sign_key().clone(),
            destination,
            tx,
        )
        .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        while rx.try_recv().is_ok() {}

        (outbound, inbound, iface, rx)
    }

    #[test]
    fn parse_link_identify_accepts_valid_proof() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let payload = build_test_identify_payload(&private, &link_id);

        let result = parse_link_identify_payload(&payload, &link_id);

        assert_eq!(result.map(|identity| identity.address_hash), Some(private.as_identity().address_hash));
    }

    #[test]
    fn parse_link_identify_rejects_every_invalid_length_without_parsing() {
        let link_id = AddressHash::new([0x01; ADDRESS_HASH_SIZE]);
        let invalid_lengths = [
            0,
            1,
            LINK_IDENTIFY_PAYLOAD_LENGTH - 1,
            LINK_IDENTIFY_PAYLOAD_LENGTH + 1,
            1024 * 1024,
        ];

        for length in invalid_lengths {
            let payload = vec![0u8; length];
            assert!(
                parse_link_identify_payload(&payload, &link_id).is_none(),
                "length {length} must be rejected"
            );
        }
    }

    #[test]
    fn parse_link_identify_rejects_corruption_in_every_payload_byte() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let valid_payload = build_test_identify_payload(&private, &link_id);

        for offset in 0..valid_payload.len() {
            let mut corrupted = valid_payload.clone();
            corrupted[offset] ^= 0x01;
            assert!(
                parse_link_identify_payload(&corrupted, &link_id).is_none(),
                "corruption at byte {offset} must be rejected"
            );
        }
    }

    #[test]
    fn parse_link_identify_binds_proof_to_the_exact_link_id() {
        let signed_link_id = AddressHash::new([0xAA; ADDRESS_HASH_SIZE]);
        let other_link_ids = [
            AddressHash::new([0x00; ADDRESS_HASH_SIZE]),
            AddressHash::new([0xAB; ADDRESS_HASH_SIZE]),
            AddressHash::new([0xFF; ADDRESS_HASH_SIZE]),
        ];
        let private = PrivateIdentity::new_from_rand(OsRng);
        let payload = build_test_identify_payload(&private, &signed_link_id);

        for link_id in other_link_ids {
            assert!(parse_link_identify_payload(&payload, &link_id).is_none());
        }
    }

    #[test]
    fn identify_packet_roundtrip_emits_only_peer_identified() {
        let (mut outbound, inbound, iface, mut rx) = linked_pair();
        let announced = PrivateIdentity::new_from_rand(OsRng);
        let payload = build_test_identify_payload(&announced, outbound.id());

        let packet = inbound.identify_packet(&payload).expect("identify packet");
        assert_eq!(packet.context, PacketContext::LinkIdentify);
        assert!(matches!(
            outbound.handle_packet(&packet, iface),
            LinkHandleResult::None
        ));

        let event = rx.try_recv().expect("peer identified event");
        match event.event {
            LinkEvent::PeerIdentified(identity) => {
                assert_eq!(identity.address_hash, announced.as_identity().address_hash);
            }
            _ => panic!("identify packet must not surface as generic link data"),
        }
        assert!(rx.try_recv().is_err(), "identify packet emitted an extra event");
    }

    #[test]
    fn repeated_invalid_identify_packets_do_not_emit_events_or_poison_the_link() {
        let (mut outbound, inbound, iface, mut rx) = linked_pair();
        let announced = PrivateIdentity::new_from_rand(OsRng);
        let valid_payload = build_test_identify_payload(&announced, outbound.id());
        let mut invalid_payload = valid_payload.clone();
        invalid_payload[PUBLIC_KEY_LENGTH * 2] ^= 0x80;
        let invalid_packet = inbound.identify_packet(&invalid_payload).expect("invalid packet build");

        for _ in 0..256 {
            assert!(matches!(
                outbound.handle_packet(&invalid_packet, iface),
                LinkHandleResult::None
            ));
        }
        assert!(rx.try_recv().is_err(), "invalid identify payload emitted an event");

        let valid_packet = inbound.identify_packet(&valid_payload).expect("valid packet build");
        assert!(matches!(
            outbound.handle_packet(&valid_packet, iface),
            LinkHandleResult::None
        ));
        assert!(matches!(
            rx.try_recv().expect("peer identified after invalid traffic").event,
            LinkEvent::PeerIdentified(_)
        ));
    }
}
