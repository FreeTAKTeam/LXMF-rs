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

    fn rtt_packet(link: &Link, value: f32, trailing_byte: bool) -> Packet {
        let mut encoded = Vec::new();
        rmp::encode::write_f32(&mut encoded, value).expect("encode RTT");
        if trailing_byte {
            encoded.push(0x00);
        }
        let mut packet = link.data_packet(&encoded).expect("encrypt RTT packet");
        packet.context = PacketContext::LinkRTT;
        packet
    }

    #[test]
    fn parse_link_identify_accepts_valid_proof() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let payload = build_test_identify_payload(&private, &link_id);

        let result = parse_link_identify_payload(&payload, &link_id);

        assert_eq!(
            result.map(|identity| identity.address_hash),
            Some(private.as_identity().address_hash)
        );
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
        let (mut outbound, mut inbound, iface, mut rx) = linked_pair();
        let link_peer_identity = *outbound.peer_identity();
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
        assert_eq!(
            outbound.identified_peer_identity().map(|identity| identity.address_hash),
            Some(announced.as_identity().address_hash),
            "verified identity must be stored before observers receive the event"
        );
        assert_eq!(
            outbound.peer_identity().address_hash,
            link_peer_identity.address_hash,
            "identification must not replace the link-session proof identity"
        );
        assert!(rx.try_recv().is_err(), "identify packet emitted an extra event");

        let data_packet = outbound.data_packet(b"proof-after-identify").expect("data packet");
        let proof = match inbound.handle_packet(&data_packet, iface) {
            LinkHandleResult::Proof(proof) => proof,
            _ => panic!("data packet should produce a proof"),
        };
        assert!(
            outbound.validate_packet_proof(&proof).is_ok(),
            "identification must not break subsequent packet proof verification"
        );
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

    #[test]
    fn invalid_identify_does_not_refresh_or_revive_a_stale_link() {
        let (mut outbound, inbound, iface, mut rx) = linked_pair();
        let announced = PrivateIdentity::new_from_rand(OsRng);
        let valid_payload = build_test_identify_payload(&announced, outbound.id());
        let mut invalid_payload = valid_payload.clone();
        invalid_payload[PUBLIC_KEY_LENGTH * 2] ^= 0x40;
        let invalid_packet = inbound.identify_packet(&invalid_payload).expect("invalid packet build");

        outbound.status = LinkStatus::Stale;
        outbound.stale_since = Some(Instant::now());
        outbound.last_inbound = None;
        outbound.last_data = None;

        assert!(matches!(
            outbound.handle_packet(&invalid_packet, iface),
            LinkHandleResult::None
        ));
        assert_eq!(outbound.status, LinkStatus::Stale);
        assert!(outbound.stale_since.is_some());
        assert!(outbound.last_inbound.is_none());
        assert!(outbound.last_data.is_none());
        assert!(rx.try_recv().is_err());

        let valid_packet = inbound.identify_packet(&valid_payload).expect("valid packet build");
        assert!(matches!(
            outbound.handle_packet(&valid_packet, iface),
            LinkHandleResult::None
        ));
        assert_eq!(outbound.status, LinkStatus::Active);
        assert!(outbound.stale_since.is_none());
        assert!(outbound.last_inbound.is_some());
        assert!(matches!(
            rx.try_recv().expect("valid identify event").event,
            LinkEvent::PeerIdentified(_)
        ));
    }

    #[test]
    fn hostile_rtt_values_and_trailing_bytes_are_rejected_without_state_change() {
        let (mut outbound, inbound, iface, _) = linked_pair();
        let baseline_rtt = outbound.rtt;
        let baseline_last_inbound = outbound.last_inbound;
        let invalid_values = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
            KEEPALIVE_MAX_SECS + 1.0,
        ];

        for value in invalid_values {
            let packet = rtt_packet(&inbound, value, false);
            assert!(matches!(
                outbound.handle_packet(&packet, iface),
                LinkHandleResult::None
            ));
            assert_eq!(outbound.rtt, baseline_rtt);
            assert_eq!(outbound.last_inbound, baseline_last_inbound);
        }

        let trailing = rtt_packet(&inbound, 0.25, true);
        assert!(matches!(
            outbound.handle_packet(&trailing, iface),
            LinkHandleResult::None
        ));
        assert_eq!(outbound.rtt, baseline_rtt);
        assert_eq!(outbound.last_inbound, baseline_last_inbound);

        let valid = rtt_packet(&inbound, 0.25, false);
        assert!(matches!(
            outbound.handle_packet(&valid, iface),
            LinkHandleResult::None
        ));
        assert!(outbound.rtt >= Duration::from_secs_f32(0.25));
        assert!(outbound.last_inbound.is_some());
    }

    #[test]
    fn malformed_keepalive_does_not_refresh_or_revive_a_stale_link() {
        let (mut outbound, inbound, iface, _) = linked_pair();
        let mut malformed = inbound.keep_alive_packet(0xFF);
        malformed.data.safe_write(&[0x00]);

        outbound.status = LinkStatus::Stale;
        outbound.stale_since = Some(Instant::now());
        outbound.last_inbound = None;

        assert!(matches!(
            outbound.handle_packet(&malformed, iface),
            LinkHandleResult::None
        ));
        assert_eq!(outbound.status, LinkStatus::Stale);
        assert!(outbound.stale_since.is_some());
        assert!(outbound.last_inbound.is_none());

        let valid = inbound.keep_alive_packet(0xFF);
        assert!(matches!(
            outbound.handle_packet(&valid, iface),
            LinkHandleResult::KeepAlive
        ));
        assert_eq!(outbound.status, LinkStatus::Active);
        assert!(outbound.stale_since.is_none());
        assert!(outbound.last_inbound.is_some());
    }

    #[test]
    fn corrupted_channel_ciphertext_is_not_acknowledged_or_counted_as_inbound() {
        let (mut outbound, inbound, iface, _) = linked_pair();
        outbound.open_channel();
        let mut packet = inbound.channel_packet(b"channel-payload").expect("channel packet");
        let last = packet.data.len() - 1;
        packet.data.as_mut_slice()[last] ^= 0x01;

        outbound.status = LinkStatus::Stale;
        outbound.stale_since = Some(Instant::now());
        outbound.last_inbound = None;

        assert!(matches!(
            outbound.handle_packet(&packet, iface),
            LinkHandleResult::None
        ));
        assert_eq!(outbound.status, LinkStatus::Stale);
        assert!(outbound.stale_since.is_some());
        assert!(outbound.last_inbound.is_none());
    }

    /// A request sent as a single packet arrives as one, and the id the
    /// responder derives is the one the requester can compute.
    ///
    /// That second half is the whole subtlety. A resource-borne request
    /// carries an id the requester chose; a packet-borne one does not, and
    /// the responder takes it from the packet hash instead. A requester
    /// that assumed its own id would be echoed back would correlate against
    /// a value the responder never saw, and every response would look
    /// unsolicited.
    #[tokio::test]
    async fn a_request_packet_round_trips_and_its_id_is_derived_from_the_packet_hash() {
        let (outbound, mut inbound, iface, mut rx) = linked_pair();

        let request = b"\x93\xcb\x00\x00\x00\x00\x00\x00\x00\x00\xc4\x04path\xc0";
        let packet = outbound.request_packet(request).expect("a request packet can be built");
        assert_eq!(packet.context, PacketContext::Request);
        let mut expected_id = [0u8; ADDRESS_HASH_SIZE];
        expected_id.copy_from_slice(&packet.hash().to_bytes()[..ADDRESS_HASH_SIZE]);

        inbound.handle_packet(&packet, iface);

        let event = rx.try_recv().expect("the request is posted to the link's event stream");
        match event.event {
            LinkEvent::Data(payload) => {
                assert_eq!(payload.as_slice(), request, "the request must survive the round trip byte for byte");
                assert_eq!(payload.context(), PacketContext::Request);
                assert_eq!(
                    payload.request_id(),
                    Some(expected_id),
                    "the responder derives the id from the packet hash — a requester has to use the same value"
                );
            }
            _ => panic!("expected the request to arrive as LinkEvent::Data"),
        }
    }

    /// A response sent as a single packet has to arrive as one — decrypted,
    /// tagged `Response`, and byte-identical to what was handed in.
    ///
    /// The receive half already handled `PacketContext::Response`; without a
    /// constructor beside `data_packet`/`identify_packet` there was no way
    /// to produce one, so every reply had to become a resource transfer even
    /// when it fit in a packet. This drives both halves across a real linked
    /// pair rather than asserting on the built packet's fields, so it fails
    /// if either side stops agreeing about the context.
    #[tokio::test]
    async fn a_response_packet_round_trips_across_a_link() {
        let (outbound, mut inbound, iface, mut rx) = linked_pair();

        // The `[request_id, response]` envelope real Reticulum packs — its
        // contents are the application's business, but the transport has to
        // carry them unchanged.
        let envelope = b"\x92\xc4\x04abcd\xc4\x05hello";
        let packet = outbound.response_packet(envelope).expect("a response packet can be built");
        assert_eq!(packet.context, PacketContext::Response);

        inbound.handle_packet(&packet, iface);

        let event = rx.try_recv().expect("the response is posted to the link's event stream");
        match event.event {
            LinkEvent::Data(payload) => {
                assert_eq!(payload.as_slice(), envelope, "the envelope must survive the round trip byte for byte");
                assert_eq!(payload.context(), PacketContext::Response, "…and still be recognisable as a response");
            }
            _ => panic!("expected the response to arrive as LinkEvent::Data"),
        }
    }

    #[tokio::test]
    async fn oversized_direct_response_is_dropped_when_request_sets_a_limit() {
        let (mut outbound, inbound, iface, mut rx) = linked_pair();
        let request = b"request";
        let packet = outbound
            .request_packet_with_max_response_size(request, Some(4))
            .expect("a bounded request packet can be built");
        let mut request_id = [0u8; ADDRESS_HASH_SIZE];
        request_id.copy_from_slice(&packet.hash().to_bytes()[..ADDRESS_HASH_SIZE]);

        let mut envelope = Vec::new();
        rmpv::encode::write_value(
            &mut envelope,
            &rmpv::Value::Array(vec![
            rmpv::Value::Binary(request_id.to_vec()),
            rmpv::Value::Binary(b"hello".to_vec()),
            ]),
        )
        .expect("response envelope");
        let response = inbound.response_packet(&envelope).expect("response packet");

        assert!(matches!(
            outbound.handle_packet(&response, iface),
            LinkHandleResult::None
        ));
        assert!(rx.try_recv().is_err(), "an oversized response must not reach consumers");
    }

    #[tokio::test]
    async fn bounded_direct_response_is_delivered_and_consumes_the_limit() {
        let (mut outbound, inbound, iface, mut rx) = linked_pair();
        let packet = outbound
            .request_packet_with_max_response_size(b"request", Some(8))
            .expect("a bounded request packet can be built");
        let mut request_id = [0u8; ADDRESS_HASH_SIZE];
        request_id.copy_from_slice(&packet.hash().to_bytes()[..ADDRESS_HASH_SIZE]);
        let mut envelope = Vec::new();
        rmpv::encode::write_value(
            &mut envelope,
            &rmpv::Value::Array(vec![
            rmpv::Value::Binary(request_id.to_vec()),
            rmpv::Value::Binary(b"hello".to_vec()),
            ]),
        )
        .expect("response envelope");

        let response = inbound.response_packet(&envelope).expect("response packet");
        outbound.handle_packet(&response, iface);
        let event = rx.try_recv().expect("bounded response should be delivered");
        assert!(matches!(event.event, LinkEvent::Data(_)));

        // The reference consumes the pending request limit with the first
        // response. A second response with the same id is therefore treated
        // as unbounded only if the caller registered a new request.
        outbound.handle_packet(&response, iface);
        assert!(rx.try_recv().is_ok(), "a later response is not blocked by a stale limit");
    }

}
