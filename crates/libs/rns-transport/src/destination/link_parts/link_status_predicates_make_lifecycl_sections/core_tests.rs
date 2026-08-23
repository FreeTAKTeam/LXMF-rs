    use super::*;

    use crate::destination::{DestinationDesc, DestinationName};

    use std::sync::{Arc, Mutex};

    #[test]
    fn link_status_predicates_make_lifecycle_edges_explicit() {
        for status in [LinkStatus::Pending, LinkStatus::Handshake] {
            assert!(status.not_yet_active());
            assert!(!status.can_exchange_data());
            assert!(!status.can_retry_channel_messages());
            assert!(!status.can_send_teardown());
        }
        assert!(LinkStatus::Active.can_exchange_data());
        assert!(LinkStatus::Active.can_retry_channel_messages());
        assert!(LinkStatus::Active.can_send_teardown());
        assert!(!LinkStatus::Stale.can_exchange_data());
        assert!(LinkStatus::Stale.can_retry_channel_messages());
        assert!(LinkStatus::Stale.can_send_teardown());
        assert!(!LinkStatus::Closed.can_exchange_data());
        assert!(!LinkStatus::Closed.can_retry_channel_messages());
        assert!(!LinkStatus::Closed.can_send_teardown());
    }

    #[test]
    fn link_close_line_preserves_compatibility_marker() {
        let line = link_close_line(&AddressHash::new([0x11; 16]));

        assert!(line.contains("link: close"));
    }

    #[test]
    fn inbound_link_request_clamps_peer_mtu_to_supported_packet_capacity() {
        let requester = PrivateIdentity::new_from_rand(OsRng);
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let mut data = PacketDataBuffer::new();
        data.safe_write(requester.as_identity().public_key.as_bytes());
        data.safe_write(requester.as_identity().verifying_key.as_bytes());
        data.safe_write(&[0x20, 0x20, 0x00]);
        let request = Packet {
            header: Header { packet_type: PacketType::LinkRequest, ..Default::default() },
            ifac: None,
            destination: destination.address_hash,
            transport: None,
            context: PacketContext::None,
            data,
        };

        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        // 0x202000 -> mode bits 0x20, MTU 8192. Honoured, not clamped: this
        // build can now carry it, and clamping it to 500 was what pinned
        // every resource fragment to 464 bytes regardless of the link.
        assert_eq!(inbound.signalling, Some([0x20, 0x20, 0x00]));
        assert_eq!(inbound.link_mtu(), 8192, "the negotiated value has to be readable back out");

        // The proof echoes the same value, so the initiator learns it too.
        let proof = inbound.prove();
        assert_eq!(
            &proof.data.as_slice()[SIGNATURE_LENGTH + PUBLIC_KEY_LENGTH..],
            &[0x20, 0x20, 0x00]
        );
    }

    /// A peer may still not talk us above what this build can carry.
    #[test]
    fn inbound_link_request_still_clamps_an_mtu_above_our_ceiling() {
        let requester = PrivateIdentity::new_from_rand(OsRng);
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let mut data = PacketDataBuffer::new();
        data.safe_write(requester.as_identity().public_key.as_bytes());
        data.safe_write(requester.as_identity().verifying_key.as_bytes());
        // Mode bits 0x20 plus the largest MTU the 21-bit field can express.
        data.safe_write(&[0x3F, 0xFF, 0xFF]);
        let request = Packet {
            header: Header { packet_type: PacketType::LinkRequest, ..Default::default() },
            ifac: None,
            destination: destination.address_hash,
            transport: None,
            context: PacketContext::None,
            data,
        };

        let inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");

        assert_eq!(inbound.link_mtu(), 262_144, "clamped to what this build will actually carry");
    }

    // Outbound-side counterpart to `inbound_link_request_clamps_peer_mtu_
    // to_supported_packet_capacity` above.
    fn decode_signalling(request: &Packet) -> (u32, u32) {
        let data = request.data.as_slice();
        let signalling = &data[data.len() - LINK_MTU_SIZE..];
        let value = ((signalling[0] as u32) << 16) | ((signalling[1] as u32) << 8) | signalling[2] as u32;
        let mode_bits = (value & LINK_MODE_MASK) >> 21;
        let mtu = value & LINK_MTU_MASK;
        (mode_bits, mtu)
    }

    fn test_link() -> Link {
        let identity = *PrivateIdentity::new_from_rand(OsRng).as_identity();
        let destination =
            DestinationDesc { identity, address_hash: identity.address_hash, name: DestinationName::new("lxmf", "delivery") };
        let (tx, _) = tokio::sync::broadcast::channel(4);
        Link::new(destination, tx)
    }

    #[test]
    fn outbound_link_request_signals_the_compiled_in_mode_and_legacy_mtu_fallback() {
        let mut link = test_link();
        let request = link.request();

        assert_eq!(request.data.len(), PUBLIC_KEY_LENGTH * 2 + LINK_MTU_SIZE);
        let (mode_bits, mtu) = decode_signalling(&request);
        assert_eq!(mode_bits, LinkMode::DEFAULT.mode_bits());
        assert_eq!(mtu, LEGACY_RETICULUM_MTU as u32);
    }

    #[test]
    fn outbound_link_request_preserves_explicit_constrained_interface_mtu() {
        let mut link = test_link();
        let request = link.request_with_mtu(384);

        let (mode_bits, mtu) = decode_signalling(&request);
        assert_eq!(mode_bits, LinkMode::DEFAULT.mode_bits());
        assert_eq!(mtu, 384);
    }

    #[test]
    fn outbound_link_request_never_changes_mode_across_repeated_attempts() {
        // The watchdog's own repeat-request cadence calls `request()`
        // again on every retry — with only one cipher ever compiled into
        // a given build (see `LinkMode`'s own doc comment), there is no
        // other mode to fall back to, so every repeat must keep signalling
        // the same, one mode this build can actually use.
        let mut link = test_link();
        for _ in 0..5 {
            let (mode_bits, _) = decode_signalling(&link.request());
            assert_eq!(mode_bits, LinkMode::DEFAULT.mode_bits());
        }
    }

    #[test]
    fn link_handshake_roundtrip_encrypts_and_decrypts() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();

        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let proof = inbound.prove();
        let proof_iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated));

        let plaintext = b"session-cached-link-payload";
        let mut cipher_buf = [0u8; PACKET_MDU];
        let ciphertext = outbound.encrypt(plaintext, &mut cipher_buf).expect("encrypt");

        let mut plain_buf = [0u8; PACKET_MDU];
        let decrypted = inbound.decrypt(ciphertext, &mut plain_buf).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn pending_request_retries_preserve_link_id() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx);
        let first_request = outbound.request();
        let first_id = *outbound.id();

        let retry_request = outbound.request();

        assert_eq!(first_id, *outbound.id());
        assert_eq!(first_id, LinkId::from(&first_request));
        assert_eq!(first_id, LinkId::from(&retry_request));
    }

    #[test]
    fn outbound_link_binds_to_proof_iface_and_rejects_other_ifaces() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();

        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let proof = inbound.prove();
        let bound_iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(outbound.handle_packet(&proof, bound_iface), LinkHandleResult::Activated));
        assert_eq!(outbound.ingress_iface(), Some(bound_iface));

        let payload = inbound.data_packet(b"hello over the right iface").expect("data packet");

        assert!(matches!(
            outbound.handle_packet(&payload, AddressHash::new_from_rand(OsRng)),
            LinkHandleResult::None
        ));
        assert!(matches!(
            outbound.handle_packet(&payload, bound_iface),
            LinkHandleResult::Proof(_)
        ));
    }

    #[test]
    fn control_context_packets_do_not_auto_generate_link_proofs() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(4);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        for context in
            [PacketContext::Request, PacketContext::Response, PacketContext::LinkIdentify]
        {
            let mut packet = inbound.data_packet(b"control-payload").expect("data packet");
            packet.context = context;
            assert!(
                matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::None),
                "{context:?} should not auto-generate a link proof"
            );
        }
    }

    #[test]
    fn channel_packets_do_not_emit_generic_link_events_and_generate_link_proofs() {
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
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        while rx.try_recv().is_ok() {}

        outbound.register_channel_handler(0xCAFE, |_| true);

        let (_sequence, packet) = inbound
            .send_channel_message(0xCAFE, b"channel-payload".to_vec())
            .expect("channel packet");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert!(rx.try_recv().is_err(), "channel packets should stay on the channel path");
    }

    #[test]
    fn channel_handlers_receive_unpacked_envelopes() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        outbound.register_channel_handler(0x1234, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        });

        let (_sequence, packet) = inbound
            .send_channel_message(0x1234, b"hello-channel".to_vec())
            .expect("channel message");
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));

        let seen = seen.lock().expect("lock");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].msg_type, 0x1234);
        assert_eq!(seen[0].payload, b"hello-channel");
    }

    #[test]
    fn channel_packets_without_open_handler_are_not_proved() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let (_sequence, packet) =
            inbound.send_channel_message(0xBEEF, b"no-handler".to_vec()).expect("channel message");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::None));
    }

    #[test]
    fn explicitly_open_channel_proves_packets_without_handlers() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        outbound.open_channel();

        let (_sequence, packet) = inbound
            .send_channel_message(0xBEEF, b"open-no-handler".to_vec())
            .expect("channel message");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
    }

    #[test]
    fn out_of_order_channel_messages_are_buffered_until_contiguous() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        outbound.register_channel_handler(0x4321, move |envelope| {
            seen_clone.lock().expect("lock").push((envelope.sequence, envelope.payload));
            true
        });

        let (_first_sequence, first_packet) =
            inbound.send_channel_message(0x4321, b"first".to_vec()).expect("first channel message");
        let (_second_sequence, second_packet) = inbound
            .send_channel_message(0x4321, b"second".to_vec())
            .expect("second channel message");

        assert!(matches!(
            outbound.handle_packet(&second_packet, iface),
            LinkHandleResult::Proof(_)
        ));
        assert!(seen.lock().expect("lock").is_empty());

        assert!(matches!(outbound.handle_packet(&first_packet, iface), LinkHandleResult::Proof(_)));

        let seen = seen.lock().expect("lock");
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, 0);
        assert_eq!(seen[0].1, b"first");
        assert_eq!(seen[1].0, 1);
        assert_eq!(seen[1].1, b"second");
    }

    #[test]
    fn duplicate_channel_messages_are_ignored() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(8);

        let mut outbound = Link::new(destination, tx.clone());
        let request = outbound.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        outbound.register_channel_handler(0x2468, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope.sequence);
            true
        });

        let (_sequence, packet) =
            inbound.send_channel_message(0x2468, b"dedupe".to_vec()).expect("channel message");

        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));
        assert!(matches!(outbound.handle_packet(&packet, iface), LinkHandleResult::Proof(_)));

        let seen = seen.lock().expect("lock");
        assert_eq!(seen.as_slice(), &[0]);
    }
