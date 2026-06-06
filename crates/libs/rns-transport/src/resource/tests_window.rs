#[test]
fn resource_receiver_slides_window_without_redundant_requests() {
    let mut link = test_requested_link();
    let mut manager = ResourceManager::new();

    const TOTAL_PARTS: usize = 10;
    let random_hash = [0xAB; RANDOM_HASH_SIZE];
    let parts: Vec<Vec<u8>> = (0..TOTAL_PARTS)
        .map(|i| vec![i as u8; PACKET_MDU])
        .collect();
    let mut hashmap_bytes = Vec::with_capacity(TOTAL_PARTS * MAPHASH_LEN);
    for part in &parts {
        hashmap_bytes.extend_from_slice(&map_hash(part, &random_hash));
    }

    let adv = ResourceAdvertisement {
        transfer_size: (TOTAL_PARTS * PACKET_MDU) as u64,
        data_size: (TOTAL_PARTS * PACKET_MDU) as u64,
        parts: TOTAL_PARTS as u32,
        hash: Hash::new_from_slice(&[0xCC; 32]),
        random_hash,
        original_hash: Hash::new_from_slice(&[0xCC; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: hashmap_bytes,
    };

    let adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("pack"),
        *link.id(),
    );
    let _ = manager.handle_packet(&adv_packet, &mut link);
    assert!(manager.incoming.contains_key(&adv.hash));

    let mut total_request_packets = 0usize;
    for part in parts.iter().take(TOTAL_PARTS - 1) {
        let packet = resource_packet(PacketContext::Resource, part, *link.id());
        let responses = manager.handle_packet(&packet, &mut link);
        let request_packets = responses
            .iter()
            .filter(|packet| packet.context == PacketContext::ResourceRequest)
            .count();
        assert!(
            request_packets <= 1,
            "expected at most 1 request per received part, got {request_packets}",
        );
        total_request_packets += request_packets;
    }

    assert!(
        total_request_packets <= TOTAL_PARTS - WINDOW,
        "total request packets {total_request_packets} exceeds TOTAL_PARTS - WINDOW = {}",
        TOTAL_PARTS - WINDOW
    );
}
