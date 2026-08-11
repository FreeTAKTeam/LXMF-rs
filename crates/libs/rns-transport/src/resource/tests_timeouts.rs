#[test]
fn resource_manager_defaults_match_reference_retry_budget() {
    let manager = ResourceManager::new();

    assert_eq!(manager.retry_interval, Duration::from_secs(2));
    assert_eq!(manager.retry_limit, 16);
}

/// Regression test for: `mark_request()` was called on every incoming part,
/// incrementing `retry_count` past `retry_limit`. The periodic
/// `retry_requests()` timer would then silently remove the receiver
/// mid-transfer once `retry_count >= retry_limit`, even though no actual
/// timeout had occurred.
///
/// The fix uses `mark_active_request()` (which does NOT increment `retry_count`)
/// in `handle_resource_part_into`. Only timer-driven retries should count.
#[test]
fn resource_receiver_not_killed_by_timer_during_active_transfer() {
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

    // Use the default config (retry_limit = 16) to catch future changes.
    let mut manager = ResourceManager::new();

    // 20 parts — more than DEFAULT_RESOURCE_MAX_RETRIES (16). Before the fix,
    // receiving 16+ parts would push retry_count to 17+ and the next timer
    // tick would remove the receiver.
    const TOTAL_PARTS: usize = 20;
    // Only send 17 parts so the transfer stays Incomplete (not Complete/Failed).
    const PARTS_TO_RECEIVE: usize = 17;

    let random_hash = [0xAB; RANDOM_HASH_SIZE];
    // Each part is distinct so every map_hash lookup finds a unique slot.
    let parts: Vec<Vec<u8>> = (0..TOTAL_PARTS)
        .map(|i| vec![i as u8; PACKET_MDU])
        .collect();

    let mut hashmap_bytes = Vec::with_capacity(TOTAL_PARTS * MAPHASH_LEN);
    for part in &parts {
        hashmap_bytes.extend_from_slice(&map_hash(part, &random_hash));
    }

    // transfer_size must satisfy max_advertised_parts >= TOTAL_PARTS.
    let transfer_size: u64 = (TOTAL_PARTS * PACKET_MDU) as u64;
    let resource_hash = Hash::new_from_slice(&[0xCC; 32]);

    let adv = ResourceAdvertisement {
        transfer_size,
        data_size: transfer_size,
        parts: TOTAL_PARTS as u32,
        hash: resource_hash,
        random_hash,
        original_hash: resource_hash,
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0, // no encryption, no compression
        hashmap: hashmap_bytes,
    };

    let adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("pack advertisement"),
        *link.id(),
    );
    let _ = manager.handle_packet(&adv_packet, &mut link);
    assert!(manager.incoming.contains_key(&resource_hash), "receiver created after advertisement");
    // After the advertisement, retry_count should be 1.
    assert_eq!(manager.incoming[&resource_hash].retry_count, 1);

    // Feed PARTS_TO_RECEIVE parts. After the fix retry_count stays at 1;
    // before the fix it would reach 1 + PARTS_TO_RECEIVE = 18 >= 16.
    for part in parts.iter().take(PARTS_TO_RECEIVE) {
        let part_packet = resource_packet(PacketContext::Resource, part, *link.id());
        manager.handle_packet(&part_packet, &mut link);
    }
    assert!(
        manager.incoming.contains_key(&resource_hash),
        "receiver still present after {PARTS_TO_RECEIVE} parts"
    );

    // Simulate the 2-second timer firing at the current moment.
    // Because parts arrived just now, retry_due() returns false (last_progress
    // and last_request are fresh), so mark_request() is NOT called here.
    // The only check is `retry_count >= retry_limit`:
    //   - After the fix:   retry_count = 1 < 16 → receiver kept  ✓
    //   - Before the fix:  retry_count = 18 >= 16 → receiver killed ✗
    let timer_now = Instant::now();
    manager.retry_requests(timer_now);
    assert!(
        manager.incoming.contains_key(&resource_hash),
        "receiver must NOT be killed by retry_requests() during active transfer"
    );

    // retry_count must be 1 (only the initial advertisement request counts).
    assert_eq!(
        manager.incoming[&resource_hash].retry_count, 1,
        "retry_count must not be incremented by incoming parts"
    );
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
fn resource_manager_receiver_cancel_emits_outbound_cancelled_event() {
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
    let (resource_hash, _) = manager
        .start_send(&link, b"cancel from receiver".to_vec(), None)
        .expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    let cancel_packet = resource_packet(
        PacketContext::ResourceReceiverCancel,
        resource_hash.as_slice(),
        *link.id(),
    );
    assert!(manager.handle_packet(&cancel_packet, &mut link).is_empty());

    assert!(!manager.outgoing.contains_key(&resource_hash));
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
    // Random, not a repeated byte — this test relies on the payload
    // splitting into exactly 2 parts (`PACKET_MDU + 32`); a uniform-byte
    // payload this size auto-compresses down to a single part, changing
    // the sender's status transitions this test asserts on.
    let mut payload = vec![0u8; PACKET_MDU + 32];
    OsRng.fill_bytes(&mut payload);
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
