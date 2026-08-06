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
        PacketContext::ResourceReceiverCancel,
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
