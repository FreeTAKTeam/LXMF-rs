#[test]
fn resource_sender_preserves_default_packet_mdu_for_large_links() {
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

    // Random, not a repeated byte — a uniform-byte payload this size is
    // trivially bz2-compressible (auto-compression now runs on every
    // outbound Resource), which would shrink this well below one full
    // MDU-sized part and defeat what this test actually checks: that a
    // payload of exactly this LENGTH is chunked at `PACKET_MDU`.
    let mut payload = vec![0u8; PACKET_MDU + 1];
    OsRng.fill_bytes(&mut payload);
    let sender = ResourceSender::new_with_options_mtu(
        &outbound,
        payload,
        None,
        None,
        false,
        DEFAULT_RESOURCE_INTERFACE_MTU,
    )
    .expect("resource sender");

    assert_eq!(sender.parts[0].len(), PACKET_MDU);
}

#[test]
fn resource_sender_constrains_parts_and_hash_updates_to_interface_mtu() {
    const LORA_MTU: usize = 220;

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

    // Random, not a repeated byte — see the sibling test's identical
    // comment above for why (auto-compression would otherwise defeat the
    // length-based chunk-count assertions below).
    let mut payload = vec![0u8; PACKET_MDU * 8];
    OsRng.fill_bytes(&mut payload);
    let mut sender = ResourceSender::new_with_options_mtu(
        &outbound,
        payload,
        None,
        None,
        false,
        LORA_MTU,
    )
    .expect("resource sender");
    assert!(
        sender.parts.len() > sender.hashmap_segment_len,
        "parts={} segment_len={}",
        sender.parts.len(),
        sender.hashmap_segment_len
    );
    assert!(sender.advertisement_packet().to_bytes().expect("advertisement wire").len() <= LORA_MTU);
    for part in &sender.parts {
        let packet =
            build_link_packet(&outbound, PacketType::Data, PacketContext::Resource, part)
                .expect("resource part packet");
        assert!(packet.to_bytes().expect("resource part wire").len() <= LORA_MTU);
    }

    let request = ResourceRequest {
        hashmap_exhausted: true,
        last_map_hash: Some(sender.map_hashes[sender.hashmap_segment_len - 1]),
        resource_hash: sender.resource_hash,
        requested_hashes: Vec::new(),
    };
    let mut responses = Vec::new();
    sender.handle_request_into(&request, &outbound, &mut responses);
    let hash_update = responses
        .into_iter()
        .find(|packet| packet.context == PacketContext::ResourceHashUpdate)
        .expect("hash update response");
    assert!(hash_update.to_bytes().expect("hash update wire").len() <= LORA_MTU);
}

#[test]
fn resource_sender_serves_only_inside_the_reticulum_collision_guard_window() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let link = Link::new(destination, tx);
    let mut payload = vec![0u8; (COLLISION_GUARD_SIZE + 4) * PACKET_MDU];
    OsRng.fill_bytes(&mut payload);
    let mut sender = ResourceSender::new(&link, payload, None).expect("resource sender");
    assert!(sender.parts.len() > COLLISION_GUARD_SIZE);

    let outside_window = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash: sender.resource_hash,
        requested_hashes: vec![sender.map_hashes[COLLISION_GUARD_SIZE + 1]],
    };
    let mut responses = Vec::new();
    sender.handle_request_into(&outside_window, &link, &mut responses);
    assert!(responses.is_empty(), "old Reticulum serving windows drop out-of-window hashes");

    let inside_window = ResourceRequest {
        requested_hashes: vec![sender.map_hashes[0]],
        ..outside_window
    };
    sender.handle_request_into(&inside_window, &link, &mut responses);
    assert_eq!(responses.len(), 1, "the active serving window must still answer requests");
}

#[test]
fn resource_sender_advances_collision_guard_anchor_at_hashmap_boundary() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let link = Link::new(destination, tx);

    // Two complete advertised hashmap segments put the requested boundary
    // past WINDOW_MAX_FAST, so the expected serving anchor is non-zero. The
    // tail extends beyond the new guard window to test both endpoints.
    let part_count = HASHMAP_MAX_LEN * 2 + COLLISION_GUARD_SIZE + 2;
    let mut payload = vec![0u8; part_count * PACKET_MDU];
    OsRng.fill_bytes(&mut payload);
    let mut sender = ResourceSender::new_with_options_mtu(
        &link,
        payload,
        None,
        None,
        false,
        DEFAULT_RESOURCE_INTERFACE_MTU,
    )
    .expect("resource sender");

    let segment_boundary = sender.hashmap_segment_len * 2;
    assert!(segment_boundary > 1 + WINDOW_MAX_FAST);
    assert!(segment_boundary - 1 < COLLISION_GUARD_SIZE);
    assert!(sender.parts.len() > segment_boundary + COLLISION_GUARD_SIZE);

    let exhausted = ResourceRequest {
        hashmap_exhausted: true,
        last_map_hash: Some(sender.map_hashes[segment_boundary - 1]),
        resource_hash: sender.resource_hash,
        requested_hashes: Vec::new(),
    };
    let mut updates = Vec::new();
    sender.handle_request_into(&exhausted, &link, &mut updates);

    let serving_start = segment_boundary - 1 - WINDOW_MAX_FAST;
    assert_eq!(sender.receiver_min_consecutive_height, serving_start);
    let update_packet = updates
        .into_iter()
        .find(|packet| packet.context == PacketContext::ResourceHashUpdate)
        .expect("the matching global part index is a hashmap boundary");
    let mut plaintext = PacketDataBuffer::new();
    let plaintext_len = {
        let decoded = link
            .decrypt(update_packet.data.as_slice(), plaintext.accuire_buf_max())
            .expect("decrypt hashmap update");
        decoded.len()
    };
    plaintext.resize(plaintext_len);
    let update = ResourceHashUpdate::decode(plaintext.as_slice()).expect("decode hashmap update");
    assert_eq!(update.segment, 2, "hashmap segments use the global part index");
    assert_eq!(
        update.hashmap,
        slice_hashmap_segment(&sender.map_hashes, 2, sender.hashmap_segment_len)
    );

    for (index, expected_packets) in [
        (serving_start - 1, 0),
        (serving_start, 1),
        (serving_start + COLLISION_GUARD_SIZE, 0),
    ] {
        let request = ResourceRequest {
            hashmap_exhausted: false,
            last_map_hash: None,
            resource_hash: sender.resource_hash,
            requested_hashes: vec![sender.map_hashes[index]],
        };
        let mut responses = Vec::new();
        sender.handle_request_into(&request, &link, &mut responses);
        assert_eq!(
            responses.len(),
            expected_packets,
            "requested hashmap index {index} must obey serving range [{serving_start}, {})",
            serving_start + COLLISION_GUARD_SIZE
        );
    }
}
