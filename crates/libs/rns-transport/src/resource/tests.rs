#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::link::LinkHandleResult;
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    /// Inter-part arrival interval used by the request tests. Small enough
    /// that `part_timeout`'s grace floor dominates, so a test that wants a
    /// timeout has to advance the clock past it deliberately.
    const TEST_ARRIVAL_INTERVAL: Duration = Duration::from_millis(1);

    #[test]
    fn resource_sender_rejects_oversized_metadata() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let link = Link::new(destination, tx);
        let data = vec![0u8; 4];
        let metadata = vec![0u8; METADATA_MAX_SIZE + 1];

        let result = ResourceSender::new(&link, data, Some(metadata));
        assert!(matches!(result, Err(RnsError::InvalidArgument)));
    }

    #[test]
    fn resource_decompression_is_bounded_by_advertised_size() {
        use bzip2::write::BzEncoder;
        use bzip2::Compression;
        use std::io::Write;

        let mut encoder = BzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(b"0123456789").expect("compress");
        let compressed = encoder.finish().expect("finish");

        assert!(decompress_resource_payload(&compressed, 9).is_err());
        assert_eq!(
            decompress_resource_payload(&compressed, 10).expect("bounded decompress"),
            b"0123456789"
        );
    }

    #[test]
    fn resource_sender_auto_compresses_a_compressible_payload_and_round_trips_through_the_receiver() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
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

        // Highly compressible: the same short phrase repeated many times —
        // this is the real-world shape this fix targets (LXMF message
        // text), not an adversarial edge case.
        let payload = b"the quick brown fox jumps over the lazy dog ".repeat(500);

        let sender = ResourceSender::new_with_options_mtu(
            &outbound,
            payload.clone(),
            None,
            None,
            false,
            DEFAULT_RESOURCE_INTERFACE_MTU,
        )
        .expect("resource sender");

        // The actual wire bytes (post-compression, post-encryption,
        // chunked) must be well under the original payload size — the
        // real, end-to-end proof that compression is genuinely wired in,
        // not just that a flag gets set somewhere.
        let wire_size: usize = sender.parts.iter().map(|part| part.len()).sum();
        assert!(
            wire_size < payload.len(),
            "wire_size ({wire_size}) should be well under the original payload size ({})",
            payload.len()
        );

        // The advertisement itself is link-encrypted (unlike Resource-
        // context parts, which carry their own separate encryption) — the
        // real Transport receive pipeline decrypts it before ever handing
        // it to `ResourceManager`, so this test does the same via the
        // existing `decrypt_advertisement` helper, then re-packs the
        // decrypted struct into a plain packet the manager can consume,
        // exactly mirroring what production code does one layer up.
        let decrypted_advertisement = decrypt_advertisement(&outbound, &sender.advertisement_packet());
        assert!(decrypted_advertisement.compressed(), "a highly compressible payload must actually get compressed");
        // The logical/decompressed size the receiver is told to expect
        // must still be the ORIGINAL payload length, not the compressed
        // wire size — a content identity, not a wire-format one (see this
        // fix's own doc comment in `sender.rs`).
        assert_eq!(decrypted_advertisement.data_size, payload.len() as u64);

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let plain_adv_packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &decrypted_advertisement.pack().expect("re-pack advertisement"),
            *inbound.id(),
        );
        let request_packets = manager.handle_packet(&plain_adv_packet, &mut inbound);
        assert_eq!(request_packets.len(), 1);

        for part in &sender.parts {
            let part_packet = resource_packet(PacketContext::Resource, part, *inbound.id());
            manager.handle_packet(&part_packet, &mut inbound);
        }

        let events = manager.drain_events();
        let complete = events
            .into_iter()
            .find_map(|event| match event.kind {
                ResourceEventKind::Complete(complete) => Some(complete),
                _ => None,
            })
            .expect("resource should complete");
        assert_eq!(complete.data, payload, "decompressed data must round-trip to the original uncompressed payload");
    }

    #[test]
    fn resource_status_predicates_make_transfer_fsm_edges_explicit() {
        for status in [
            ResourceStatus::None,
            ResourceStatus::Advertised,
            ResourceStatus::Transferring,
            ResourceStatus::AwaitingProof,
        ] {
            assert!(!status.is_terminal());
        }
        for status in [ResourceStatus::Complete, ResourceStatus::Failed] {
            assert!(status.is_terminal());
        }
        for status in [
            ResourceStatus::Advertised,
            ResourceStatus::Transferring,
            ResourceStatus::AwaitingProof,
        ] {
            assert!(status.accepts_transfer_activity());
        }
        for status in [ResourceStatus::None, ResourceStatus::Complete, ResourceStatus::Failed] {
            assert!(!status.accepts_transfer_activity());
        }
    }

    #[test]
    fn resource_sender_marks_request_and_response_advertisements() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
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

        let request_id = vec![0xAA; ADDRESS_HASH_SIZE];
        let request_sender = ResourceSender::new_with_options(
            &outbound,
            b"request".to_vec(),
            None,
            Some(request_id.clone()),
            false,
        )
        .expect("request sender");
        let request_adv = decrypt_advertisement(&outbound, &request_sender.advertisement_packet());
        assert_eq!(
            request_adv.request_id.as_ref().map(|id| id.as_ref()),
            Some(request_id.as_slice())
        );
        assert!(request_adv.is_request());
        assert!(!request_adv.is_response());

        let response_sender = ResourceSender::new_with_options(
            &outbound,
            b"response".to_vec(),
            None,
            Some(request_id.clone()),
            true,
        )
        .expect("response sender");
        let response_adv =
            decrypt_advertisement(&outbound, &response_sender.advertisement_packet());
        assert_eq!(
            response_adv.request_id.as_ref().map(|id| id.as_ref()),
            Some(request_id.as_slice())
        );
        assert!(response_adv.is_response());
        assert!(!response_adv.is_request());
    }

    #[test]
    fn resource_manager_rejects_inconsistent_split_metadata() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let adv = ResourceAdvertisement {
            transfer_size: 1,
            data_size: 1,
            parts: 1,
            hash: Hash::new_from_slice(&[1, 2, 3, 4]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[1, 2, 3, 4]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: FLAG_SPLIT,
            hashmap: vec![0u8; MAPHASH_LEN],
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let responses = manager.handle_packet(&packet, &mut link);

        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
    }

    #[test]
    fn resource_sender_sequences_split_advertisements_after_proofs() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
        let data = vec![0x5a; MAX_EFFICIENT_SIZE + 257];

        let (original_hash, first_packet) =
            manager.start_send(&link, data, None).expect("start split resource");
        let first = decrypt_advertisement(&link, &first_packet);
        assert_eq!(first.hash, original_hash);
        assert_eq!(first.original_hash, original_hash);
        assert_eq!(first.segment_index, 1);
        assert_eq!(first.total_segments, 2);
        assert_eq!(first.flags & FLAG_SPLIT, FLAG_SPLIT);
        manager.confirm_outbound_dispatch(original_hash, true);

        let first_proof = manager.outgoing.get(&first.hash).expect("first sender").expected_proof;
        let proof = ResourceProof { resource_hash: first.hash, proof: first_proof };
        let next_packets = manager.handle_packet(
            &resource_packet(PacketContext::ResourceProof, &proof.encode(), *link.id()),
            &mut link,
        );
        assert_eq!(next_packets.len(), 1);
        let second = decrypt_advertisement(&link, &next_packets[0]);
        assert_eq!(second.original_hash, original_hash);
        assert_eq!(second.segment_index, 2);
        assert_eq!(second.total_segments, 2);

        let second_proof = manager
            .outgoing
            .get(&second.hash)
            .expect("second sender")
            .expected_proof;
        let proof = ResourceProof { resource_hash: second.hash, proof: second_proof };
        assert!(manager
            .handle_packet(
                &resource_packet(PacketContext::ResourceProof, &proof.encode(), *link.id()),
                &mut link,
            )
            .is_empty());
        let events = manager.drain_events();
        assert!(events.iter().any(|event| {
            event.hash == original_hash && matches!(event.kind, ResourceEventKind::OutboundComplete)
        }));
    }

    #[test]
    fn resource_receiver_assembles_ordered_split_segments() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();
        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
        let first_data = b"first-segment";
        let second_data = b"second-segment";

        let total_data_size = (first_data.len() + second_data.len()) as u64;
        let (first_adv, first_part) =
            split_test_segment(first_data, None, 1, 2, total_data_size);
        let original_hash = first_adv.hash;
        let first_packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &first_adv.pack().expect("first advertisement"),
            *link.id(),
        );
        assert_eq!(manager.handle_packet(&first_packet, &mut link).len(), 1);
        assert_eq!(
            manager
                .handle_packet(
                    &resource_packet(PacketContext::Resource, &first_part, *link.id()),
                    &mut link,
                )
                .len(),
            1
        );
        assert!(manager
            .drain_events()
            .iter()
            .all(|event| !matches!(event.kind, ResourceEventKind::Complete(_))));

        let (second_adv, second_part) =
            split_test_segment(second_data, Some(original_hash), 2, 2, total_data_size);
        let second_packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &second_adv.pack().expect("second advertisement"),
            *link.id(),
        );
        assert_eq!(manager.handle_packet(&second_packet, &mut link).len(), 1);
        let responses = manager.handle_packet(
            &resource_packet(PacketContext::Resource, &second_part, *link.id()),
            &mut link,
        );
        let events = manager.drain_events();
        assert_eq!(responses.len(), 1, "events={events:?}");

        let complete = events
            .into_iter()
            .find_map(|event| match event.kind {
                ResourceEventKind::Complete(complete) => Some((event.hash, complete)),
                _ => None,
            })
            .expect("assembled split resource");
        assert_eq!(complete.0, original_hash);
        assert_eq!(complete.1.data, [first_data.as_slice(), second_data.as_slice()].concat());
    }

    /// Issue #520: out-of-order split segments are logged and dropped (not
    /// accepted, not RCL-cancelled), and the transfer recovers once the
    /// expected segment arrives.
    #[test]
    fn resource_receiver_drops_out_of_order_split_segments_until_sequence_resumes() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();
        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
        let first_data = b"first-segment";
        let second_data = b"second-segment";

        let total_data_size = (first_data.len() + second_data.len()) as u64;
        let (first_adv, first_part) =
            split_test_segment(first_data, None, 1, 2, total_data_size);
        let original_hash = first_adv.hash;
        let (second_adv, second_part) =
            split_test_segment(second_data, Some(original_hash), 2, 2, total_data_size);

        // Segment 2 advertisement arrives before segment 1: dropped, with
        // no request issued and no receiver state created.
        let early_second = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &second_adv.pack().expect("second advertisement"),
            *link.id(),
        );
        assert!(manager.handle_packet(&early_second, &mut link).is_empty());
        assert!(manager.incoming.is_empty(), "out-of-order segment must not create a receiver");

        // Segment 1 arrives and is accepted normally.
        let first_packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &first_adv.pack().expect("first advertisement"),
            *link.id(),
        );
        assert_eq!(manager.handle_packet(&first_packet, &mut link).len(), 1);
        assert_eq!(
            manager
                .handle_packet(
                    &resource_packet(PacketContext::Resource, &first_part, *link.id()),
                    &mut link,
                )
                .len(),
            1
        );

        // A stale replay of segment 1 (expected segment is now 2) is also
        // dropped without tearing anything down.
        assert!(manager.handle_packet(&first_packet, &mut link).is_empty());

        // Segment 2, now in order, is accepted and assembly completes.
        let second_packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &second_adv.pack().expect("second advertisement"),
            *link.id(),
        );
        assert_eq!(manager.handle_packet(&second_packet, &mut link).len(), 1);
        manager.handle_packet(
            &resource_packet(PacketContext::Resource, &second_part, *link.id()),
            &mut link,
        );

        let complete = manager
            .drain_events()
            .into_iter()
            .find_map(|event| match event.kind {
                ResourceEventKind::Complete(complete) => Some((event.hash, complete)),
                _ => None,
            })
            .expect("split resource should assemble once order resumes");
        assert_eq!(complete.0, original_hash);
        assert_eq!(complete.1.data, [first_data.as_slice(), second_data.as_slice()].concat());
    }

    #[test]
    fn resource_manager_ignores_duplicate_active_advertisement() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let part = b"hello-resource";
        let random_hash = [7u8; RANDOM_HASH_SIZE];
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: part.len() as u64,
            parts: 1,
            hash: Hash::new_from_slice(&[9u8; 32]),
            random_hash,
            original_hash: Hash::new_from_slice(&[9u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap,
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let first = manager.handle_packet(&packet, &mut link);
        assert_eq!(first.len(), 1);
        assert_eq!(manager.incoming.len(), 1);
        assert_eq!(
            manager.incoming.get(&adv.hash).expect("receiver").retry_count,
            1
        );

        let second = manager.handle_packet(&packet, &mut link);
        assert!(second.is_empty());
        assert_eq!(manager.incoming.len(), 1);
        assert_eq!(
            manager.incoming.get(&adv.hash).expect("receiver").retry_count,
            1
        );
    }

    #[test]
    fn resource_receiver_uses_advertised_hashmap_stride_for_updates() {
        let random_hash = [3u8; RANDOM_HASH_SIZE];
        let parts = [
            b"part-00".as_slice(),
            b"part-01".as_slice(),
            b"part-02".as_slice(),
            b"part-03".as_slice(),
        ];
        let map_hashes: Vec<[u8; MAPHASH_LEN]> =
            parts.iter().map(|part| map_hash(part, &random_hash)).collect();
        let mut first_segment = Vec::with_capacity(MAPHASH_LEN * 2);
        first_segment.extend_from_slice(&map_hashes[0]);
        first_segment.extend_from_slice(&map_hashes[1]);
        let mut second_segment = Vec::with_capacity(MAPHASH_LEN * 2);
        second_segment.extend_from_slice(&map_hashes[2]);
        second_segment.extend_from_slice(&map_hashes[3]);
        let adv = ResourceAdvertisement {
            transfer_size: parts.iter().map(|part| part.len() as u64).sum(),
            data_size: parts.iter().map(|part| part.len() as u64).sum(),
            parts: parts.len() as u32,
            hash: Hash::new_from_slice(&[9u8; 32]),
            random_hash,
            original_hash: Hash::new_from_slice(&[9u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: first_segment,
        };

        let mut receiver =
            ResourceReceiver::new(&adv, AddressHash::new_from_slice(&[4u8; ADDRESS_HASH_SIZE]))
                .expect("receiver accepts smaller advertised hashmap segment");
        assert_eq!(receiver.hashmap_segment_len, 2);

        receiver.handle_hash_update(&ResourceHashUpdate {
            resource_hash: adv.hash,
            segment: 1,
            hashmap: second_segment,
        });

        assert_eq!(receiver.hashmap, map_hashes.into_iter().map(Some).collect::<Vec<_>>());
    }

    /// Builds a receiver for a resource whose hashmap needs several
    /// segments, with only segment zero known — the state every large
    /// transfer starts in.
    fn multi_segment_receiver(total_parts: usize, segment_len: usize) -> (ResourceReceiver, Vec<[u8; MAPHASH_LEN]>) {
        let random_hash = [7u8; RANDOM_HASH_SIZE];
        let bodies: Vec<Vec<u8>> = (0..total_parts).map(|i| format!("part-{i:05}").into_bytes()).collect();
        let map_hashes: Vec<[u8; MAPHASH_LEN]> =
            bodies.iter().map(|body| map_hash(body, &random_hash)).collect();
        let mut first_segment = Vec::with_capacity(MAPHASH_LEN * segment_len);
        for hash in map_hashes.iter().take(segment_len) {
            first_segment.extend_from_slice(hash);
        }
        let adv = ResourceAdvertisement {
            transfer_size: bodies.iter().map(|body| body.len() as u64).sum(),
            data_size: bodies.iter().map(|body| body.len() as u64).sum(),
            parts: total_parts as u32,
            hash: Hash::new_from_slice(&[11u8; 32]),
            random_hash,
            original_hash: Hash::new_from_slice(&[11u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: first_segment,
        };
        let receiver =
            ResourceReceiver::new(&adv, AddressHash::new_from_slice(&[5u8; ADDRESS_HASH_SIZE]))
                .expect("advertisement with a partial hashmap is valid");
        (receiver, map_hashes)
    }

    /// Exhaustion means "the fragments I want next are unmapped", not "some
    /// fragment somewhere is unmapped".
    ///
    /// The difference is not cosmetic. Signalling exhaustion makes the
    /// reference sender advance `receiver_min_consecutive_height` by a whole
    /// hashmap segment (`RNS/Resource.py`), and it only serves fragments
    /// inside `parts[that .. that + COLLISION_GUARD_SIZE]`. Signalling it on
    /// every request walks that window off the end of the fragments actually
    /// being requested, and the sender then drops them silently.
    ///
    /// Measured against a real NomadNet node before this fix: 8 fragments of
    /// 2260 arrived, followed by 266 hashmap-update packets and no further
    /// data until the transfer timed out.
    #[test]
    fn a_mapped_request_window_does_not_report_hashmap_exhaustion() {
        let segment_len = 74;
        let (mut receiver, _) = multi_segment_receiver(600, segment_len);

        let request = receiver.build_request(Instant::now(), Duration::from_millis(50), TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);

        assert!(
            !request.hashmap_exhausted,
            "the first {WINDOW} fragments are mapped by segment zero — nothing to ask the sender for"
        );
        assert_eq!(request.requested_hashes.len(), WINDOW, "the window should be full of real fragment requests");
    }

    /// Once the map *is* exhausted, exactly one request goes out and the
    /// receiver waits. Without the gate it re-asks at link RTT, and each
    /// re-ask moves the reference sender's serving window forward again.
    #[test]
    fn an_outstanding_hashmap_update_suppresses_further_requests() {
        let segment_len = 2;
        let (mut receiver, map_hashes) = multi_segment_receiver(8, segment_len);
        let now = Instant::now();

        // Nothing received yet, so the window is [0, WINDOW) and segment
        // zero maps only its first two slots — the state a large transfer
        // is in from its very first request.
        let first = receiver.build_request(now, Duration::from_millis(50), TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
        assert!(first.hashmap_exhausted, "the next fragment is unmapped, so the map has to be asked for");
        assert_eq!(
            first.last_map_hash,
            Some(map_hashes[segment_len - 1]),
            "the sender matches this against its own parts and cancels the transfer if it is not a segment boundary"
        );

        let second = receiver.build_request(now, Duration::from_millis(50), TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
        assert!(!second.hashmap_exhausted, "asking twice for the same segment is what walks the sender's window off");
        assert!(second.requested_hashes.is_empty(), "…and there is nothing else to ask for either");

        // The update lands: the receiver resumes immediately.
        let mut segment_bytes = Vec::new();
        for hash in map_hashes.iter().skip(segment_len).take(segment_len) {
            segment_bytes.extend_from_slice(hash);
        }
        receiver.handle_hash_update(&ResourceHashUpdate {
            resource_hash: receiver.resource_hash,
            segment: 1,
            hashmap: segment_bytes,
        });

        let third = receiver.build_request(now, Duration::from_millis(50), TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
        assert_eq!(
            third.requested_hashes,
            map_hashes[segment_len..segment_len * 2].to_vec(),
            "the newly mapped fragments are requested straight away"
        );
        assert!(!third.hashmap_exhausted, "and the window they fill is mapped again");
    }

    /// The window opens as rounds succeed, instead of sitting at four for
    /// the whole transfer.
    ///
    /// Four fragments per round is the reference's *starting* window; it
    /// grows toward `WINDOW_MAX_SLOW` and, on a link measured fast enough,
    /// to `WINDOW_MAX_FAST`. Holding it at four caps throughput at four
    /// fragments per round trip no matter how good the link is — on a
    /// 2260-fragment resource that is 565 sequential round trips.
    #[test]
    fn the_request_window_opens_as_rounds_succeed() {
        let (mut receiver, map_hashes) = multi_segment_receiver(600, 600);
        let mut now = Instant::now();
        let rtt = Duration::from_millis(50);

        assert_eq!(receiver.build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate).requested_hashes.len(), WINDOW, "the first round is the starting window");

        // Complete round after round, delivering exactly what was asked for.
        let mut delivered = 0usize;
        let mut sizes = Vec::new();
        for _ in 0..6 {
            while delivered < receiver.consecutive_completed_height + receiver.window {
                receiver.parts[delivered] = Some(b"body".to_vec());
                receiver.received += 1;
                receiver.in_flight_set.remove(&delivered);
                delivered += 1;
            }
            receiver.consecutive_completed_height = delivered;
            receiver.note_round_complete(now);
            now += rtt;
            sizes.push(receiver.build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate).requested_hashes.len());
        }

        assert!(
            sizes.windows(2).all(|pair| pair[1] >= pair[0]),
            "a window that shrinks on success would be worse than a fixed one: {sizes:?}"
        );
        assert!(sizes.last().unwrap() > &WINDOW, "six clean rounds must buy more than the starting window: {sizes:?}");
        assert!(
            sizes.iter().all(|size| *size <= WINDOW_MAX_SLOW),
            "…but not past the slow ceiling until the link measures fast: {sizes:?}"
        );
        let _ = map_hashes;
    }

    /// Growth has to survive a pipeline that never empties.
    ///
    /// This receiver refills the window on every part rather than draining
    /// it, so a "nothing in flight" trigger starves itself: the bigger the
    /// window, the rarer a full drain, and growth stops exactly where it
    /// matters. Measured against a real node that ceiling was 42 against an
    /// allowed 75 — with 97% of rounds showing the window completely full,
    /// so the window, not the link, was the limit.
    #[test]
    fn the_window_keeps_growing_while_the_pipeline_stays_full() {
        let (mut receiver, _) = multi_segment_receiver(4000, 4000);
        let mut now = Instant::now();

        // Deliver fragments steadily, never letting the window drain.
        for _ in 0..3000 {
            now += Duration::from_millis(1);
            receiver.received_bytes += 464;
            receiver.note_fragment_received(now);
        }

        assert_eq!(
            receiver.window, WINDOW_MAX_FAST,
            "a steadily-delivering link must reach the ceiling, not stall below it"
        );
    }

    /// …and closes again when fragments go missing, once per round rather
    /// than once per fragment.
    ///
    /// A window of four that times out wholesale is one failed round. Four
    /// separate shrinks would collapse straight to the floor and discard
    /// everything the link had proven.
    #[test]
    fn a_round_of_losses_narrows_the_window_once() {
        let (mut receiver, _) = multi_segment_receiver(600, 600);
        let now = Instant::now();
        let rtt = Duration::from_millis(50);

        // Open the window up first, so there is room to shrink.
        for _ in 0..5 {
            receiver.note_round_complete(now);
        }
        let opened = receiver.window;
        assert!(opened > WINDOW_MIN, "precondition: the window has room to close");

        receiver.build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
        // Every fragment just requested times out together.
        let much_later = now + rtt * 100;
        receiver.build_request(much_later, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);

        assert_eq!(receiver.window, opened - 1, "one failed round, one step back — not one per fragment");
        assert!(receiver.window >= WINDOW_MIN);
    }

    /// The fast ceiling exists, is reached only by sustained measurement,
    /// and stops exactly where the sender's serving window assumes it will.
    #[test]
    fn a_sustained_fast_link_unlocks_the_fast_ceiling_and_no_further() {
        let (mut receiver, _) = multi_segment_receiver(600, 600);
        let mut now = Instant::now();

        // Rounds that move plenty of data in very little time.
        for _ in 0..FAST_RATE_THRESHOLD {
            receiver.received_bytes += 100_000;
            now += Duration::from_millis(10);
            receiver.note_round_complete(now);
        }
        assert_eq!(receiver.window_max, WINDOW_MAX_FAST, "sustained fast rounds unlock the fast ceiling");

        for _ in 0..500 {
            now += Duration::from_millis(10);
            receiver.received_bytes += 100_000;
            receiver.note_round_complete(now);
        }
        assert_eq!(
            receiver.window, WINDOW_MAX_FAST,
            "the window stops at the sender's own WINDOW_MAX — asking past it means asking for fragments \
             a reference sender has already stopped serving"
        );
    }

    /// A lost hashmap update must not park the transfer forever.
    ///
    /// It would, and silently: `retry_count` only advances when a request is
    /// actually sent, so a permanently-gated receiver is never even declared
    /// failed. This is issue #369's failure mode exactly.
    #[test]
    fn a_lost_hashmap_update_is_re_requested_rather_than_hanging() {
        let segment_len = 2;
        let (mut receiver, _) = multi_segment_receiver(8, segment_len);
        let now = Instant::now();
        let rtt = Duration::from_millis(50);

        assert!(receiver.build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate).hashmap_exhausted);
        receiver.mark_request();

        let still_waiting = receiver.build_request(now + Duration::from_millis(10), rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
        assert!(!still_waiting.hashmap_exhausted, "a reply is still plausibly in flight");

        let gave_up = receiver.build_request(now + hashmap_update_wait(rtt) + Duration::from_secs(1), rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
        assert!(gave_up.hashmap_exhausted, "the update never came — ask again rather than wait forever");
    }

    #[test]
    fn resource_completion_preserves_request_response_metadata() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let data = b"file-response";
        let random_hash = [0x66; RANDOM_HASH_SIZE];
        let mut part = Vec::with_capacity(RANDOM_HASH_SIZE + data.len());
        part.extend_from_slice(&random_hash);
        part.extend_from_slice(data);
        let resource_hash = Hash::new_from_slice(&[data.as_slice(), &random_hash].concat());
        let request_id = vec![0x44; ADDRESS_HASH_SIZE];
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(&part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: data.len() as u64,
            parts: 1,
            hash: resource_hash,
            random_hash,
            original_hash: resource_hash,
            segment_index: 1,
            total_segments: 1,
            request_id: Some(ByteBuf::from(request_id.clone())),
            flags: FLAG_RESPONSE,
            hashmap,
        };
        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let request_packets = manager.handle_packet(&packet, &mut link);
        assert_eq!(request_packets.len(), 1);
        let part_packet = resource_packet(PacketContext::Resource, &part, *link.id());
        let proof_packets = manager.handle_packet(&part_packet, &mut link);
        assert_eq!(proof_packets.len(), 1);

        let events = manager.drain_events();
        let complete = events
            .into_iter()
            .find_map(|event| match event.kind {
                ResourceEventKind::Complete(complete) => Some(complete),
                _ => None,
            })
            .expect("resource complete event");
        assert_eq!(complete.data, data);
        assert_eq!(complete.request_id.as_deref(), Some(request_id.as_slice()));
        assert!(complete.is_response);
        assert!(!complete.is_request);
    }

    #[test]
    fn resource_manager_removes_failed_receiver_without_followup_request() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let part = b"not-bzip";
        let random_hash = [5u8; RANDOM_HASH_SIZE];
        let resource_hash = Hash::new_from_slice(&[8u8; 32]);
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: part.len() as u64,
            parts: 1,
            hash: resource_hash,
            random_hash,
            original_hash: resource_hash,
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: FLAG_COMPRESSED,
            hashmap,
        };

        let adv_packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let first = manager.handle_packet(&adv_packet, &mut link);
        assert_eq!(first.len(), 1);
        assert_eq!(manager.incoming.len(), 1);

        let part_packet = resource_packet(PacketContext::Resource, part, *link.id());
        let responses = manager.handle_packet(&part_packet, &mut link);
        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
        let events = manager.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].hash, resource_hash);
        assert_eq!(events[0].link_id, *link.id());
        let ResourceEventKind::InboundFailed(failure) = &events[0].kind else {
            panic!("expected inbound failure event");
        };
        assert_eq!(failure.reason, "decompress_failed");
        assert_eq!(failure.progress.received_parts, 1);
    }

    #[test]
    fn resource_receiver_reports_failure_reason() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let part = b"not-bzip";
        let random_hash = [5u8; RANDOM_HASH_SIZE];
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
        let adv = ResourceAdvertisement {
            transfer_size: part.len() as u64,
            data_size: part.len() as u64,
            parts: 1,
            hash: Hash::new_from_slice(&[8u8; 32]),
            random_hash,
            original_hash: Hash::new_from_slice(&[8u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: FLAG_COMPRESSED,
            hashmap,
        };
        let mut receiver = ResourceReceiver::new(&adv, *link.id()).expect("resource receiver");

        assert!(matches!(
            receiver.handle_part(part, &link),
            PartOutcome::Failed("decompress_failed")
        ));
    }

    // Regression tests for issue #514: the manager must reject
    // advertisements exceeding MAX_INBOUND_RESOURCE_TRANSFER_SIZE or
    // MAX_INBOUND_RESOURCE_PARTS before any receiver (and its
    // part-tracking allocations) is created.
    fn advertisement_with(transfer_size: u64, data_size: u64, parts: u32) -> ResourceAdvertisement {
        ResourceAdvertisement {
            transfer_size,
            data_size,
            parts,
            hash: Hash::new_from_slice(&[3u8; 32]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[3u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: vec![0u8; MAPHASH_LEN],
        }
    }

    fn manager_with_test_link() -> (ResourceManager, Link) {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();
        (ResourceManager::new_with_config(Duration::from_secs(1), 1), link)
    }

    #[test]
    fn resource_manager_rejects_advertisement_over_transfer_size_limit() {
        let (mut manager, mut link) = manager_with_test_link();
        // A multi-gigabyte advertised transfer must never reach receiver
        // creation; a transfer exactly at the cap still must.
        let oversized =
            advertisement_with(MAX_INBOUND_RESOURCE_TRANSFER_SIZE + 1, 1, 1);
        let packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &oversized.pack().expect("advertisement"),
            *link.id(),
        );

        let responses = manager.handle_packet(&packet, &mut link);
        assert!(responses.is_empty(), "rejected advertisement must not produce a request");
        assert!(manager.incoming.is_empty(), "rejected advertisement must not create a receiver");

        let at_limit = advertisement_with(MAX_INBOUND_RESOURCE_TRANSFER_SIZE, 1, 1);
        let packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &at_limit.pack().expect("advertisement"),
            *link.id(),
        );
        let responses = manager.handle_packet(&packet, &mut link);
        assert!(!manager.incoming.is_empty(), "transfer at the limit is still accepted");
        assert!(!responses.is_empty(), "accepted advertisement requests parts");
    }

    #[test]
    fn resource_manager_rejects_advertisement_over_parts_limit() {
        let (mut manager, mut link) = manager_with_test_link();
        // An excessive part count must never reach receiver creation; a
        // count exactly at the cap still must.
        let oversized = advertisement_with(1, 1, (MAX_INBOUND_RESOURCE_PARTS + 1) as u32);
        let packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &oversized.pack().expect("advertisement"),
            *link.id(),
        );

        let responses = manager.handle_packet(&packet, &mut link);
        assert!(responses.is_empty(), "rejected advertisement must not produce a request");
        assert!(manager.incoming.is_empty(), "rejected advertisement must not create a receiver");

        let at_limit = advertisement_with(1, 1, 1);
        let packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &at_limit.pack().expect("advertisement"),
            *link.id(),
        );
        let responses = manager.handle_packet(&packet, &mut link);
        assert!(!manager.incoming.is_empty(), "valid advertisement is still accepted");
        assert!(!responses.is_empty());
    }

    #[test]
    fn resource_receiver_rejects_unreasonable_advertised_parts() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let adv = ResourceAdvertisement {
            transfer_size: 1,
            data_size: 1,
            parts: 2,
            hash: Hash::new_from_slice(&[3u8; 32]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[3u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: vec![0u8; MAPHASH_LEN * 2],
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let responses = manager.handle_packet(&packet, &mut link);

        assert!(responses.is_empty());
        assert!(manager.incoming.is_empty());
    }

    #[test]
    fn resource_receiver_accepts_sender_with_smaller_effective_sdu() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "resource"),
        };
        let (tx, _) = tokio::sync::broadcast::channel(1);
        let mut link = Link::new(destination, tx);
        link.request();

        let advertised_parts = 16;
        let adv = ResourceAdvertisement {
            transfer_size: PACKET_MDU as u64 + 1,
            data_size: PACKET_MDU as u64 + 1,
            parts: advertised_parts,
            hash: Hash::new_from_slice(&[4u8; 32]),
            random_hash: [0u8; RANDOM_HASH_SIZE],
            original_hash: Hash::new_from_slice(&[4u8; 32]),
            segment_index: 1,
            total_segments: 1,
            request_id: None,
            flags: 0,
            hashmap: vec![0u8; MAPHASH_LEN * advertised_parts as usize],
        };

        let packet =
            resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());

        let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
        let responses = manager.handle_packet(&packet, &mut link);

        assert_eq!(responses.len(), 1);
        assert_eq!(manager.incoming.len(), 1);
    }

    #[test]
    fn resource_receiver_bounds_part_count_by_transfer_size_and_global_cap() {
        assert_eq!(max_advertised_parts(1, PACKET_MDU).expect("one byte transfer"), 1);
        assert_eq!(
            max_advertised_parts(PACKET_MDU as u64, PACKET_MDU).expect("one packet transfer"),
            PACKET_MDU as u64
        );
        assert_eq!(
            max_advertised_parts(PACKET_MDU as u64 + 1, PACKET_MDU)
                .expect("larger transfer"),
            PACKET_MDU as u64 + 1
        );
        assert!(max_advertised_parts(0, PACKET_MDU).is_err());
        assert!(max_advertised_parts(MAX_INBOUND_RESOURCE_TRANSFER_SIZE + 1, PACKET_MDU).is_err());
        assert_eq!(
            max_advertised_parts(MAX_INBOUND_RESOURCE_TRANSFER_SIZE, PACKET_MDU)
                .expect("maximum transfer"),
            MAX_INBOUND_RESOURCE_PARTS
        );
    }

    include!("tests_mtu.rs");
    include!("tests_retry_failures.rs");
    include!("tests_hashmap_gate.rs");
    include!("tests_window_rounds.rs");
    include!("tests_split_assembly.rs");
    include!("tests_split_lazy.rs");
    include!("tests_split_metadata.rs");
    include!("tests_timeouts.rs");
    include!("tests_timeouts_cleanup.rs");
    include!("tests_timeouts_lifecycle.rs");

    fn resource_packet(context: PacketContext, payload: &[u8], destination: AddressHash) -> Packet {
        Packet {
            header: Header {
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination,
            context,
            data: PacketDataBuffer::new_from_slice(payload),
            ..Default::default()
        }
    }

    fn split_test_segment(
        data: &[u8],
        original_hash: Option<Hash>,
        segment_index: u32,
        total_segments: u32,
        total_data_size: u64,
    ) -> (ResourceAdvertisement, Vec<u8>) {
        let random_hash = segment_index.to_be_bytes();
        let mut part = Vec::with_capacity(RANDOM_HASH_SIZE + data.len());
        part.extend_from_slice(&random_hash);
        part.extend_from_slice(data);
        let resource_hash = Hash::new_from_slice(&[data, random_hash.as_slice()].concat());
        let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
        hashmap.extend_from_slice(&map_hash(&part, &random_hash));
        (
            ResourceAdvertisement {
                transfer_size: part.len() as u64,
                data_size: total_data_size,
                parts: 1,
                hash: resource_hash,
                random_hash,
                original_hash: original_hash.unwrap_or(resource_hash),
                segment_index,
                total_segments,
                request_id: None,
                flags: FLAG_SPLIT,
                hashmap,
            },
            part,
        )
    }

    fn decrypt_advertisement(link: &Link, packet: &Packet) -> ResourceAdvertisement {
        let mut buffer = PacketDataBuffer::new();
        let plain_len = {
            let plain =
                link.decrypt(packet.data.as_slice(), buffer.accuire_buf_max()).expect("decrypt adv");
            plain.len()
        };
        buffer.resize(plain_len);
        ResourceAdvertisement::unpack(buffer.as_slice()).expect("advertisement")
    }
}
