#[test]
fn request_scoped_cleanup_survives_crypto_failure_and_preserves_other_transfers() {
    let (mut manager, mut link) = manager_with_test_link();
    link.prove();
    let link_id = *link.id();
    let (original, _) = manager
        .start_send(&link, vec![3; MAX_EFFICIENT_SIZE + 1], None)
        .expect("split request Resource");
    manager.confirm_outbound_dispatch(original, true);
    let (other, _) = manager.start_send(&link, vec![4], None).expect("unrelated pending Resource");
    let request_id = [3; 16];
    let mut response = advertisement_with(1, 1, 1);
    response.flags |= FLAG_RESPONSE;
    response.request_id = Some(request_id.to_vec().into());
    let response_hash = response.hash;
    manager.handle_packet(
        &resource_packet(
            PacketContext::ResourceAdvrtisement,
            &response.pack().expect("response advertisement"),
            link_id,
        ),
        &mut link,
    );
    assert!(manager.has_incoming_resource(link_id, response_hash));
    manager.drain_events();
    let inbound = std::collections::HashSet::from([response_hash]);
    let mut failures = 0;
    let packets = manager.cancel_scoped_state(
        CleanupScope::Request {
            link_id,
            request_id: &request_id,
            outbound: Some(original),
            inbound: &inbound,
        },
        Some(&link),
        |_, _, _| {
            failures += 1;
            Err(RnsError::CryptoError)
        },
    );
    assert_eq!(failures, 2, "both outgoing and incoming frame construction failed");
    assert!(packets.is_empty());
    assert!(!manager.outgoing.contains_key(&original));
    assert!(!manager.outgoing_segment_chains.contains_key(&original));
    assert!(!manager.has_incoming_resource(link_id, response_hash));
    assert!(manager.pending_outgoing.contains_key(&other));
    let events = manager.drain_events();
    assert_eq!(events.len(), 2, "native cleanup still reports both logical outcomes");
    assert!(events.iter().any(|event| event.hash == original
        && matches!(event.kind, ResourceEventKind::OutboundCancelled)));
    assert!(events.iter().any(|event| event.hash == response_hash
        && matches!(event.kind, ResourceEventKind::InboundFailed(_))));
}

#[test]
fn request_cleanup_removes_accepted_split_receiver_and_assembly_only_for_its_hashes() {
    let (mut manager, mut link) = manager_with_test_link();
    link.prove();
    let link_id = *link.id();
    let request_id = [3; 16];
    let mut originals = Vec::new();
    for data in [b"request first".as_slice(), b"unrelated first".as_slice()] {
        let (mut advertisement, part) = split_test_segment(data, None, 1, 2, 30);
        if originals.is_empty() {
            advertisement.flags |= FLAG_RESPONSE;
            advertisement.request_id = Some(request_id.to_vec().into());
        }
        originals.push(advertisement.hash);
        manager.handle_packet(
            &resource_packet(
                PacketContext::ResourceAdvrtisement,
                &advertisement.pack().expect("first advertisement"),
                link_id,
            ),
            &mut link,
        );
        manager.handle_packet(&resource_packet(PacketContext::Resource, &part, link_id), &mut link);
    }
    assert_eq!(manager.incoming_segments.len(), 2);
    let original = originals[0];
    let (mut second, _) = split_test_segment(b"request second", Some(original), 2, 2, 30);
    second.flags |= FLAG_RESPONSE;
    second.request_id = Some(request_id.to_vec().into());
    manager.handle_packet(
        &resource_packet(
            PacketContext::ResourceAdvrtisement,
            &second.pack().expect("second advertisement"),
            link_id,
        ),
        &mut link,
    );
    manager.drain_events();
    let inbound = std::collections::HashSet::from([original, second.hash]);
    let packets = manager.cancel_request_state(link_id, &request_id, None, &inbound, Some(&link));
    assert_eq!(packets.len(), 1, "only the active segment needs a cancellation frame");
    assert!(!manager.incoming.contains_key(&second.hash));
    assert!(!manager.incoming_segments.contains_key(&original));
    assert!(manager.incoming_segments.contains_key(&originals[1]));
    let events = manager.drain_events();
    assert_eq!(events.len(), 1, "active receiver and partial assembly are one logical failure");
    assert_eq!(events[0].hash, original);
    assert!(matches!(events[0].kind, ResourceEventKind::InboundFailed(_)));
}

#[test]
fn response_segment_cannot_attach_to_an_ordinary_assembly_or_authorize_its_cleanup() {
    use crate::destination::link::{RequestSource, RequestStatus};
    let (mut manager, mut link) = manager_with_test_link();
    link.prove();
    let link_id = *link.id();
    let request_id = [3; 16];
    let (receipt, mut dispatch) = link
        .register_request(
            request_id.to_vec(),
            RequestSource::Packet(Hash::new_from_slice(b"request")),
            None,
            None,
        )
        .expect("request owner");
    dispatch.confirm_handoff(true);
    dispatch.returned();
    let (first, part) = split_test_segment(b"first", None, 1, 2, 11);
    manager.handle_packet(
        &resource_packet(
            PacketContext::ResourceAdvrtisement,
            &first.pack().expect("first advertisement"),
            link_id,
        ),
        &mut link,
    );
    manager.handle_packet(&resource_packet(PacketContext::Resource, &part, link_id), &mut link);
    assert!(manager.incoming_segments.contains_key(&first.hash));
    let (second, part) = split_test_segment(b"second", Some(first.hash), 2, 2, 11);
    let mut relabelled = second.clone();
    relabelled.flags |= FLAG_RESPONSE;
    relabelled.request_id = Some(request_id.to_vec().into());
    manager.handle_packet(
        &resource_packet(
            PacketContext::ResourceAdvrtisement,
            &relabelled.pack().expect("forged second advertisement"),
            link_id,
        ),
        &mut link,
    );
    assert_eq!(receipt.status(), RequestStatus::Sent);
    assert!(!manager.incoming.contains_key(&second.hash));
    // Even a forged cleanup hash set cannot authorize removal of ordinary state.
    manager.cancel_request_state(
        link_id,
        &request_id,
        None,
        &std::collections::HashSet::from([first.hash, second.hash]),
        Some(&link),
    );
    assert!(manager.incoming_segments.contains_key(&first.hash));
    manager.drain_events();
    manager.handle_packet(
        &resource_packet(
            PacketContext::ResourceAdvrtisement,
            &second.pack().expect("valid second advertisement"),
            link_id,
        ),
        &mut link,
    );
    manager.handle_packet(&resource_packet(PacketContext::Resource, &part, link_id), &mut link);
    assert!(manager.drain_events().iter().any(|event| matches!(&event.kind, ResourceEventKind::Complete(complete) if complete.data == b"firstsecond" && !complete.is_response)));
}
