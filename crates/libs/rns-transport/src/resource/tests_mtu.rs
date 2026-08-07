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
