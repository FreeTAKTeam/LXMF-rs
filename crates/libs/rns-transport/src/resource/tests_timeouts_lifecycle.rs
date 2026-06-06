#[test]
fn resource_sender_emits_outbound_failed_when_status_is_failed() {
    // Covers the new `ResourceStatus::Failed => OutboundResourcePoll::Failed` arm
    // in poll().  The same path is exercised when handle_request_into() fails to
    // build a packet and sets self.status = Failed.
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
        manager.start_send(&link, b"fail me".to_vec(), None).expect("start sender");
    manager.confirm_outbound_dispatch(resource_hash, true);

    assert!(manager.outgoing.contains_key(&resource_hash));
    assert!(manager.drain_events().is_empty());

    manager.outgoing.get_mut(&resource_hash).unwrap().status = ResourceStatus::Failed;

    let packets = manager.poll_outgoing(Instant::now());
    assert!(packets.is_empty());
    assert!(!manager.outgoing.contains_key(&resource_hash));

    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].hash, resource_hash);
    assert_eq!(events[0].link_id, *link.id());
    assert!(matches!(events[0].kind, ResourceEventKind::OutboundFailed));
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
