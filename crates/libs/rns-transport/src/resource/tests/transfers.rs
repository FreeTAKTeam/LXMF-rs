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
fn compression_threshold_does_not_cap_outbound_resource_admission() {
    use std::io::{Repeat, Take};

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
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(outbound.handle_packet(&inbound.prove(), iface), LinkHandleResult::Activated));

    const CAP: u64 = 64 * 1024 * 1024;
    let at_limit_reader: Take<Repeat> = std::io::repeat(b'R').take(CAP);
    let prepared = ResourceManager::prepare_send_from_reader(
        &outbound,
        at_limit_reader,
        CAP,
        None,
        None,
        false,
        DEFAULT_RESOURCE_INTERFACE_MTU,
        true,
    )
    .expect("production reader sender admits exactly the compression cap");
    let mut manager = ResourceManager::new();
    let (_, advertisement_packet) = manager.track_prepared(prepared);
    assert_eq!(advertisement_packet.context, PacketContext::ResourceAdvrtisement);
    let advertisement = decrypt_advertisement(&outbound, &advertisement_packet);
    assert_eq!(advertisement.data_size, CAP);
    assert_eq!(advertisement.total_segments, 65);
    assert!(advertisement.compressed(), "compressible first segment is advertised compressed");
    assert!(manager.drain_events().is_empty(), "preparation is not reported as success");

    let above_limit = ResourceManager::prepare_send_from_reader(
        &outbound,
        std::io::repeat(b'R').take(CAP + 1),
        CAP + 1,
        None,
        None,
        false,
        DEFAULT_RESOURCE_INTERFACE_MTU,
        true,
    )
    .expect("the compression threshold is not an outbound admission limit");
    let mut manager = ResourceManager::new();
    let (_, advertisement_packet) = manager.track_prepared(above_limit);
    let advertisement = decrypt_advertisement(&outbound, &advertisement_packet);
    assert_eq!(advertisement.data_size, CAP + 1);
    assert_eq!(advertisement.total_segments, 65);
    assert!(!advertisement.compressed());
    assert!(manager.drain_events().is_empty());
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
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(outbound.handle_packet(&inbound.prove(), iface), LinkHandleResult::Activated));

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
    assert!(
        decrypted_advertisement.compressed(),
        "a highly compressible payload must actually get compressed"
    );
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
    assert_eq!(
        complete.data, payload,
        "decompressed data must round-trip to the original uncompressed payload"
    );
}

#[test]
fn split_resource_compresses_first_segment_with_metadata() {
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
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(outbound.handle_packet(&inbound.prove(), iface), LinkHandleResult::Activated));

    let data = vec![b'R'; MAX_EFFICIENT_SIZE * 2];
    let metadata = rmp_serde::to_vec(&"python-meta").expect("encode metadata");
    let prepared = ResourceManager::prepare_send_with_compression(
        &outbound,
        data,
        Some(metadata),
        None,
        false,
        DEFAULT_RESOURCE_INTERFACE_MTU,
        true,
    )
    .expect("prepare split metadata Resource");
    let advertisement = decrypt_advertisement(&outbound, &prepared.first.advertisement_packet());
    assert!(advertisement.compressed());
    assert_eq!(advertisement.total_segments, 3);
}

#[test]
fn resource_sender_can_disable_opportunistic_compression_for_binary_responses() {
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
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(outbound.handle_packet(&inbound.prove(), iface), LinkHandleResult::Activated));

    let payload = b"already encoded image bytes ".repeat(500);
    let metadata = b"{\"name\":\"image.webp\"}".to_vec();
    let sender = ResourceSender::new_with_options_mtu_and_compression(
        &outbound,
        payload.clone(),
        Some(metadata.clone()),
        Some(vec![0x42; 16]),
        true,
        DEFAULT_RESOURCE_INTERFACE_MTU,
        false,
    )
    .expect("resource sender");
    let advertisement = decrypt_advertisement(&outbound, &sender.advertisement_packet());
    assert!(!advertisement.compressed());
    assert_eq!(advertisement.data_size, (payload.len() + 3 + metadata.len()) as u64);
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
    for status in
        [ResourceStatus::Advertised, ResourceStatus::Transferring, ResourceStatus::AwaitingProof]
    {
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
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(outbound.handle_packet(&inbound.prove(), iface), LinkHandleResult::Activated));

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
    assert_eq!(request_adv.request_id.as_ref().map(|id| id.as_ref()), Some(request_id.as_slice()));
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
    let response_adv = decrypt_advertisement(&outbound, &response_sender.advertisement_packet());
    assert_eq!(response_adv.request_id.as_ref().map(|id| id.as_ref()), Some(request_id.as_slice()));
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

    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *link.id(),
    );

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

    let second_proof = manager.outgoing.get(&second.hash).expect("second sender").expected_proof;
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
    let (first_adv, first_part) = split_test_segment(first_data, None, 1, 2, total_data_size);
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
    let (first_adv, first_part) = split_test_segment(first_data, None, 1, 2, total_data_size);
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

    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *link.id(),
    );

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let first = manager.handle_packet(&packet, &mut link);
    assert_eq!(first.len(), 1);
    assert_eq!(manager.incoming.len(), 1);
    assert_eq!(manager.incoming.get(&adv.hash).expect("receiver").retry_count, 1);

    let second = manager.handle_packet(&packet, &mut link);
    assert!(second.is_empty());
    assert_eq!(manager.incoming.len(), 1);
    assert_eq!(manager.incoming.get(&adv.hash).expect("receiver").retry_count, 1);
}
