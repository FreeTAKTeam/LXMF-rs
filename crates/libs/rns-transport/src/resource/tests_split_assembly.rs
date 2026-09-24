/// Every path that abandons a split-resource assembly has to say so. Returning
/// quietly leaves the caller blocked on a transfer the manager already knows is
/// dead, until an unrelated timeout expires (issue #369).
#[test]
fn resource_manager_reports_a_split_resource_that_assembles_to_the_wrong_size() {
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
    // One byte more than the segments actually deliver, so the final size check
    // rejects the assembly.
    let total_data_size = (first_data.len() + second_data.len() + 1) as u64;

    let (first_adv, first_part) = split_test_segment(first_data, None, 1, 2, total_data_size);
    let original_hash = first_adv.hash;
    let (second_adv, second_part) =
        split_test_segment(second_data, Some(original_hash), 2, 2, total_data_size);

    for (adv, part) in [(first_adv, first_part), (second_adv, second_part)] {
        let adv_packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &adv.pack().expect("pack advertisement"),
            *link.id(),
        );
        assert_eq!(manager.handle_packet(&adv_packet, &mut link).len(), 1);
        let part_packet = resource_packet(PacketContext::Resource, &part, *link.id());
        assert_eq!(manager.handle_packet(&part_packet, &mut link).len(), 1);
    }

    assert!(!manager.incoming_segments.contains_key(&original_hash));
    let events = manager.drain_events();
    assert!(
        !events.iter().any(|event| matches!(event.kind, ResourceEventKind::Complete(_))),
        "a short assembly must not complete"
    );
    let failure = events
        .iter()
        .find_map(|event| match &event.kind {
            ResourceEventKind::InboundFailed(failure) if event.hash == original_hash => {
                Some(failure)
            }
            _ => None,
        })
        .expect("inbound failure for the abandoned assembly");
    assert_eq!(failure.reason, "assembled_size_mismatch");
    assert_eq!(failure.progress.total_parts, 2);
}

#[test]
fn remote_cancel_clears_the_partial_split_assembly_and_reports_failure() {
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
    let (second_adv, _) =
        split_test_segment(second_data, Some(original_hash), 2, 2, total_data_size);

    let first_adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &first_adv.pack().expect("pack first advertisement"),
        *link.id(),
    );
    assert_eq!(manager.handle_packet(&first_adv_packet, &mut link).len(), 1);
    let first_part_packet = resource_packet(PacketContext::Resource, &first_part, *link.id());
    assert_eq!(manager.handle_packet(&first_part_packet, &mut link).len(), 1);

    let second_adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &second_adv.pack().expect("pack second advertisement"),
        *link.id(),
    );
    assert_eq!(manager.handle_packet(&second_adv_packet, &mut link).len(), 1);
    assert!(manager.incoming_segments.contains_key(&original_hash));
    assert!(manager.incoming.contains_key(&second_adv.hash));

    let cancel_packet = resource_packet(
        PacketContext::ResourceInitiatorCancel,
        second_adv.hash.as_slice(),
        *link.id(),
    );
    assert!(manager.handle_packet(&cancel_packet, &mut link).is_empty());
    assert!(!manager.incoming.contains_key(&second_adv.hash));
    assert!(!manager.incoming_segments.contains_key(&original_hash));

    let events = manager.drain_events();
    let failure = events
        .iter()
        .find_map(|event| match &event.kind {
            ResourceEventKind::InboundFailed(failure) if event.hash == original_hash => {
                Some(failure)
            }
            _ => None,
        })
        .expect("remote cancellation should report an inbound failure");
    assert_eq!(failure.reason, "remote_cancelled");
    assert_eq!(failure.progress.received_parts, 1);
    assert_eq!(failure.progress.total_parts, 2);
}

#[test]
fn failed_later_split_segment_clears_partial_assembly_and_reports_original_hash() {
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
    let expected_tail = b"invalid-tail";
    let total_data_size = (first_data.len() + expected_tail.len()) as u64;
    let (first_adv, first_part) = split_test_segment(first_data, None, 1, 2, total_data_size);
    let original_hash = first_adv.hash;
    let first_adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &first_adv.pack().expect("pack first advertisement"),
        *link.id(),
    );
    assert_eq!(manager.handle_packet(&first_adv_packet, &mut link).len(), 1);
    let first_part_packet = resource_packet(PacketContext::Resource, &first_part, *link.id());
    assert_eq!(manager.handle_packet(&first_part_packet, &mut link).len(), 1);
    assert!(manager.incoming_segments.contains_key(&original_hash));
    let _ = manager.drain_events();

    let random_hash = [2u8; RANDOM_HASH_SIZE];
    let mut invalid_compressed_part = random_hash.to_vec();
    invalid_compressed_part.extend_from_slice(b"not-a-bz2-stream");
    let mut segment_hash_input = invalid_compressed_part.clone();
    segment_hash_input.extend_from_slice(&random_hash);
    let segment_hash = Hash::new_from_slice(&segment_hash_input);
    let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
    hashmap.extend_from_slice(&map_hash(&invalid_compressed_part, &random_hash));
    let second_adv = ResourceAdvertisement {
        transfer_size: invalid_compressed_part.len() as u64,
        data_size: total_data_size,
        parts: 1,
        hash: segment_hash,
        random_hash,
        original_hash,
        segment_index: 2,
        total_segments: 2,
        request_id: None,
        flags: FLAG_SPLIT | FLAG_COMPRESSED,
        hashmap,
    };
    let second_adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &second_adv.pack().expect("pack second advertisement"),
        *link.id(),
    );
    assert_eq!(manager.handle_packet(&second_adv_packet, &mut link).len(), 1);
    let failed_part_packet =
        resource_packet(PacketContext::Resource, &invalid_compressed_part, *link.id());

    let _ = manager.handle_packet(&failed_part_packet, &mut link);

    assert!(manager.incoming.is_empty());
    assert!(manager.incoming_segments.is_empty());
    let events = manager.drain_events();
    assert_eq!(events.len(), 1, "split failure emits one terminal event");
    assert_eq!(events[0].hash, original_hash);
    let ResourceEventKind::InboundFailed(failure) = &events[0].kind else {
        panic!("expected terminal inbound failure");
    };
    assert_eq!(failure.reason, "decompress_failed");
}

#[test]
fn split_retry_exhaustion_clears_partial_assembly() {
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

    let first_data = b"first-segment";
    let second_data = b"second-segment";
    let total_data_size = (first_data.len() + second_data.len()) as u64;
    let (first_adv, first_part) = split_test_segment(first_data, None, 1, 2, total_data_size);
    let original_hash = first_adv.hash;
    let (second_adv, _) =
        split_test_segment(second_data, Some(original_hash), 2, 2, total_data_size);
    for adv in [first_adv, second_adv] {
        let packet = resource_packet(
            PacketContext::ResourceAdvrtisement,
            &adv.pack().expect("pack split advertisement"),
            *link.id(),
        );
        assert_eq!(manager.handle_packet(&packet, &mut link).len(), 1);
        if adv.segment_index == 1 {
            let part = resource_packet(PacketContext::Resource, &first_part, *link.id());
            assert_eq!(manager.handle_packet(&part, &mut link).len(), 1);
            let _ = manager.drain_events();
        }
    }
    assert!(manager.incoming_segments.contains_key(&original_hash));

    let _ = manager.retry_requests(Instant::now() + Duration::from_secs(2));

    assert!(manager.incoming.is_empty());
    assert!(manager.incoming_segments.is_empty());
    let events = manager.drain_events();
    assert_eq!(events.len(), 1, "retry exhaustion emits one terminal event");
    assert_eq!(events[0].hash, original_hash);
    let ResourceEventKind::InboundFailed(failure) = &events[0].kind else {
        panic!("expected terminal inbound failure");
    };
    assert_eq!(failure.reason, "retry_limit_exhausted");
}
