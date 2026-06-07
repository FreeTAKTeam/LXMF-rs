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

    let sender = ResourceSender::new_with_options_mtu(
        &outbound,
        vec![0x42; PACKET_MDU + 1],
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

    let mut sender = ResourceSender::new_with_options_mtu(
        &outbound,
        vec![0x42; PACKET_MDU * 8],
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
