#[test]
fn resource_manager_emits_inbound_failed_when_retry_budget_exhausts() {
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

    let part = b"never-arrives";
    let random_hash = [0xAB; RANDOM_HASH_SIZE];
    let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
    hashmap.extend_from_slice(&map_hash(part, &random_hash));
    let resource_hash = Hash::new_from_slice(&[0xCC; 32]);
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
        flags: 0,
        hashmap,
    };

    let adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("pack advertisement"),
        *link.id(),
    );
    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let _ = manager.handle_packet(&adv_packet, &mut link);

    manager.retry_requests(Instant::now() + Duration::from_secs(2));

    assert!(!manager.incoming.contains_key(&resource_hash));
    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].hash, resource_hash);
    let ResourceEventKind::InboundFailed(failure) = &events[0].kind else {
        panic!("expected inbound failure event");
    };
    assert_eq!(failure.reason, "retry_limit_exhausted");
    assert_eq!(failure.progress.received_parts, 0);
}
