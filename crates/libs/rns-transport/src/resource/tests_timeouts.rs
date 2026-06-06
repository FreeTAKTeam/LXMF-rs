#[test]
fn resource_manager_defaults_match_reference_retry_budget() {
    let manager = ResourceManager::new();

    assert_eq!(manager.retry_interval, Duration::from_secs(2));
    assert_eq!(manager.retry_limit, 16);
}

#[test]
fn resource_advertisements_use_reference_advertisement_retry_budget() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let link = Link::new(destination, tx);

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 16);
    let (resource_hash, _) =
        manager.start_send(&link, b"retry me".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let sender = manager.outgoing.get(&resource_hash).expect("outgoing sender");
    assert_eq!(sender.max_retries, 16);
    assert_eq!(sender.retries_left, 4);
}

#[test]
fn resource_manager_retries_advertisement_until_budget_exhausted() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let link = Link::new(destination, tx);

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
    let (resource_hash, _) =
        manager.start_send(&link, b"retry me".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let now = Instant::now() + Duration::from_secs(2);
    let first = manager.poll_outgoing(now);
    assert_eq!(first.len(), 1);
    assert!(manager.outgoing.contains_key(&resource_hash));

    let second = manager.poll_outgoing(now + Duration::from_secs(2));
    assert_eq!(second.len(), 1);
    assert!(manager.outgoing.contains_key(&resource_hash));

    let third = manager.poll_outgoing(now + Duration::from_secs(4));
    assert!(third.is_empty());
    assert!(!manager.outgoing.contains_key(&resource_hash));
}

#[test]
fn resource_manager_emits_outbound_failed_when_advertisement_retry_budget_exhausts() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let link = Link::new(destination, tx);

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let (resource_hash, _) =
        manager.start_send(&link, b"retry me".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let now = Instant::now() + Duration::from_secs(2);
    let retry = manager.poll_outgoing(now);
    assert_eq!(retry.len(), 1);
    assert!(manager.drain_events().is_empty());

    let exhausted = manager.poll_outgoing(now + Duration::from_secs(2));
    assert!(exhausted.is_empty());
    assert!(!manager.outgoing.contains_key(&resource_hash));
    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].hash, resource_hash);
    assert_eq!(events[0].link_id, *link.id());
    assert!(matches!(events[0].kind, ResourceEventKind::OutboundFailed));
}

#[test]
fn resource_manager_emits_outbound_failed_when_advertisement_dispatch_fails() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let link = Link::new(destination, tx);

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let (resource_hash, _) =
        manager.start_send(&link, b"dispatch fail".to_vec(), None).expect("start sender");

    manager.confirm_outbound_dispatch(resource_hash, false);

    assert!(manager.pending_outgoing.is_empty());
    assert!(manager.outgoing.is_empty());
    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].hash, resource_hash);
    assert_eq!(events[0].link_id, *link.id());
    assert!(matches!(events[0].kind, ResourceEventKind::OutboundFailed));
}

#[test]
fn resource_manager_cancel_outgoing_emits_initiator_cancel_packet_and_event() {
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
        manager.start_send(&link, b"cancel me".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let cancel_packet = manager
        .cancel_outgoing(resource_hash, &link)
        .expect("cancel packet")
        .expect("active sender cancel frame");

    assert!(!manager.outgoing.contains_key(&resource_hash));
    assert_eq!(cancel_packet.destination, *link.id());
    assert_eq!(cancel_packet.context, PacketContext::ResourceInitiatorCancel);
    let mut decrypted = PacketDataBuffer::new();
    let plain_len = {
        let plain = link
            .decrypt(cancel_packet.data.as_slice(), decrypted.accuire_buf_max())
            .expect("decrypt cancel packet");
        plain.len()
    };
    decrypted.resize(plain_len);
    assert_eq!(decrypted.as_slice(), resource_hash.as_slice());

    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].hash, resource_hash);
    assert_eq!(events[0].link_id, *link.id());
    assert!(matches!(events[0].kind, ResourceEventKind::OutboundCancelled));
}

#[test]
fn resource_manager_times_out_transferring_sender_after_retry_budget() {
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

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let payload = vec![0x42; PACKET_MDU + 32];
    let (resource_hash, _) = manager.start_send(&link, payload, None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let first_map_hash = manager
        .outgoing
        .get(&resource_hash)
        .expect("outgoing sender")
        .map_hashes[0];
    let request = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash,
        requested_hashes: vec![first_map_hash],
    };
    let request_packet =
        resource_packet(PacketContext::ResourceRequest, &request.encode(), *link.id());
    let responses = manager.handle_packet(&request_packet, &mut link);

    assert_eq!(responses.len(), 1);
    assert_eq!(
        manager.outgoing.get(&resource_hash).expect("sender").status,
        ResourceStatus::Transferring
    );

    let now = Instant::now() + Duration::from_secs(2);
    let first = manager.poll_outgoing(now);
    assert!(first.is_empty());
    assert!(manager.outgoing.contains_key(&resource_hash));

    let second = manager.poll_outgoing(now + Duration::from_secs(2));
    assert!(second.is_empty());
    assert!(!manager.outgoing.contains_key(&resource_hash));
}

#[test]
fn resource_manager_times_out_awaiting_proof_after_retry_budget() {
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

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let (resource_hash, _) =
        manager.start_send(&link, b"proof please".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let first_map_hash = manager
        .outgoing
        .get(&resource_hash)
        .expect("outgoing sender")
        .map_hashes[0];
    let request = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash,
        requested_hashes: vec![first_map_hash],
    };
    let request_packet =
        resource_packet(PacketContext::ResourceRequest, &request.encode(), *link.id());
    let responses = manager.handle_packet(&request_packet, &mut link);

    assert_eq!(responses.len(), 1);
    assert_eq!(
        manager.outgoing.get(&resource_hash).expect("sender").status,
        ResourceStatus::AwaitingProof
    );

    let now = Instant::now() + Duration::from_secs(2);
    let first = manager.poll_outgoing(now);
    assert!(first.is_empty());
    assert!(manager.outgoing.contains_key(&resource_hash));

    let second = manager.poll_outgoing(now + Duration::from_secs(2));
    assert!(second.is_empty());
    assert!(!manager.outgoing.contains_key(&resource_hash));
}

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
}

#[test]
fn resource_receiver_sends_no_redundant_requests_during_fast_transfer() {
    // Before fix: N parts arriving → N request packets (one per Incomplete outcome).
    // After fix:  N parts arriving in <<50ms → 0 request packets (cooldown not elapsed).
    // The remaining<WINDOW guard is also exercised: parts 8-9 leave 2-1 parts
    // remaining (<WINDOW=4), so immediate_request_due returns false regardless of time.
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

    // Feed 9 of 10 parts in a tight loop — all arrive within microseconds.
    let mut request_count = 0usize;
    for part in parts.iter().take(TOTAL_PARTS - 1) {
        let p = resource_packet(PacketContext::Resource, part, *link.id());
        let responses = manager.handle_packet(&p, &mut link);
        request_count += responses
            .iter()
            .filter(|p| p.context == PacketContext::ResourceRequest)
            .count();
    }

    assert_eq!(
        request_count, 0,
        "expected no redundant request packets during fast transfer, got {request_count}"
    );
}

#[test]
fn resource_manager_link_close_allows_later_resource_on_new_link() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let mut first_link = Link::new(destination, tx.clone());
    first_link.request();

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 2);
    let (first_hash, _) =
        manager.start_send(&first_link, vec![0x11; PACKET_MDU + 24], None).expect("first send");
    manager.confirm_outbound_dispatch(first_hash, true);

    let adv = ResourceAdvertisement {
        transfer_size: 1,
        data_size: 1,
        parts: 1,
        hash: Hash::new_from_slice(&[0x44; 32]),
        random_hash: [0u8; RANDOM_HASH_SIZE],
        original_hash: Hash::new_from_slice(&[0x44; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: vec![0u8; MAPHASH_LEN],
    };
    let incoming_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *first_link.id(),
    );
    let _ = manager.handle_packet(&incoming_packet, &mut first_link);
    manager.remove_link_state(*first_link.id());

    let mut second_link = Link::new(destination, tx);
    second_link.request();
    let (second_hash, _) =
        manager.start_send(&second_link, vec![0x22; PACKET_MDU + 24], None).expect("second send");
    manager.confirm_outbound_dispatch(second_hash, true);

    assert!(!manager.outgoing.contains_key(&first_hash));
    assert!(manager.outgoing.contains_key(&second_hash));
    assert!(manager.incoming.is_empty());

    let first_map_hash = manager
        .outgoing
        .get(&second_hash)
        .expect("second outgoing sender")
        .map_hashes[0];
    let request = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash: second_hash,
        requested_hashes: vec![first_map_hash],
    };
    let request_packet =
        resource_packet(PacketContext::ResourceRequest, &request.encode(), *second_link.id());
    let responses = manager.handle_packet(&request_packet, &mut second_link);

    assert_eq!(responses.len(), 1);
    assert_eq!(
        manager.outgoing.get(&second_hash).expect("second sender").status,
        ResourceStatus::Transferring
    );
}
