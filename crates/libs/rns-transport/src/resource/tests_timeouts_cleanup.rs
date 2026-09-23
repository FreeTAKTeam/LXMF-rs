#[test]
fn resource_manager_removes_link_scoped_state_on_link_close() {
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
    let (resource_hash, _) =
        manager.start_send(&link, b"cleanup".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let adv = ResourceAdvertisement {
        transfer_size: 1,
        data_size: 1,
        parts: 1,
        hash: Hash::new_from_slice(&[0x33; 32]),
        random_hash: [0u8; RANDOM_HASH_SIZE],
        original_hash: Hash::new_from_slice(&[0x33; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: vec![0u8; MAPHASH_LEN],
    };
    let packet =
        resource_packet(PacketContext::ResourceAdvrtisement, &adv.pack().expect("advertisement"), *link.id());
    let _ = manager.handle_packet(&packet, &mut link);

    manager.remove_link_state(*link.id());

    assert!(manager.pending_outgoing.is_empty());
    assert!(manager.outgoing.is_empty());
    assert!(manager.incoming.is_empty());
    let events = manager.drain_events();
    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|event| {
        event.hash == resource_hash
            && matches!(event.kind, ResourceEventKind::OutboundFailed)
    }));
    assert!(events.iter().any(|event| {
        event.hash == adv.hash
            && matches!(event.kind, ResourceEventKind::InboundFailed(_))
    }));
}

#[test]
fn resource_manager_exhausts_missing_fragment_retries_after_partial_progress() {
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

    let parts = [b"received-part".as_slice(), b"missing-part".as_slice()];
    let random_hash = [0x6D; RANDOM_HASH_SIZE];
    let mut hashmap = Vec::with_capacity(parts.len() * MAPHASH_LEN);
    for part in parts {
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
    }
    let resource_hash = Hash::new_from_slice(&[0x71; HASH_SIZE]);
    let advertisement = ResourceAdvertisement {
        transfer_size: parts.iter().map(|part| part.len() as u64).sum(),
        data_size: parts.iter().map(|part| part.len() as u64).sum(),
        parts: parts.len() as u32,
        hash: resource_hash,
        random_hash,
        original_hash: resource_hash,
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap,
    };
    let advertisement_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &advertisement.pack().expect("pack advertisement"),
        *link.id(),
    );
    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    assert!(!manager.handle_packet(&advertisement_packet, &mut link).is_empty());

    let received_packet = resource_packet(PacketContext::Resource, parts[0], *link.id());
    let _ = manager.handle_packet(&received_packet, &mut link);
    assert!(manager.incoming.contains_key(&resource_hash));

    // Python RNS/Resource.py cancels a receiver once its missing-part retry
    // budget is exhausted. Advance the deterministic manager clock past the
    // same retry interval; no wall-clock sleeps or relaxed outcome matching.
    let failures = manager.retry_requests(Instant::now() + Duration::from_secs(2));
    assert!(failures.is_empty(), "terminal failure is emitted as an event");
    assert!(!manager.incoming.contains_key(&resource_hash));
    let events = manager.drain_events();
    let failures: Vec<_> = events
        .iter()
        .filter(|event| matches!(event.kind, ResourceEventKind::InboundFailed(_)))
        .collect();
    assert_eq!(failures.len(), 1, "exactly one terminal failure is emitted");
    let failure_event = failures[0];
    assert_eq!(failure_event.hash, resource_hash);
    assert_eq!(failure_event.link_id, *link.id());
    let ResourceEventKind::InboundFailed(failure) = &failure_event.kind else {
        unreachable!("filtered to inbound failure events");
    };
    assert_eq!(failure.reason, "retry_limit_exhausted");
    assert_eq!(failure.progress.received_parts, 1);
}

#[test]
fn resource_receiver_slides_window_without_redundant_requests() {
    // With adaptive in-flight tracking the receiver keeps at most WINDOW fragments
    // in flight at any time. Each received part opens one slot, so exactly one new
    // fragment is requested per arrived part once the pipeline is full — never the
    // same fragment twice while it is still in flight.
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

    // Feed 9 of 10 parts. Verify window-sliding behaviour:
    // at most 1 new request per received part (window opens by 1 slot each time),
    // and the total number of request packets is bounded by TOTAL_PARTS - WINDOW
    // (WINDOW fragments were already requested in the advertisement response).
    let mut total_request_packets = 0usize;
    for part in parts.iter().take(TOTAL_PARTS - 1) {
        let p = resource_packet(PacketContext::Resource, part, *link.id());
        let responses = manager.handle_packet(&p, &mut link);
        let req_packets: Vec<_> = responses
            .iter()
            .filter(|p| p.context == PacketContext::ResourceRequest)
            .collect();
        assert!(
            req_packets.len() <= 1,
            "expected at most 1 request per received part, got {}",
            req_packets.len()
        );
        total_request_packets += req_packets.len();
    }

    assert!(
        total_request_packets <= TOTAL_PARTS - WINDOW,
        "total request packets {} exceeds TOTAL_PARTS - WINDOW = {}",
        total_request_packets,
        TOTAL_PARTS - WINDOW
    );
}
