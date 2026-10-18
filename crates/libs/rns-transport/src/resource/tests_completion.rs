#[tokio::test]
async fn resource_completion_tracks_logical_failure_cancel_and_rejection() {
    for segment in [0, 1, 2] {
        for outcome in
            [ResourceOutcome::Failed, ResourceOutcome::Cancelled, ResourceOutcome::Rejected]
        {
            let (mut manager, mut link) = manager_with_test_link();
            let (original, _) = manager
                .start_send(&link, vec![0x51; MAX_EFFICIENT_SIZE + 1], None)
                .expect("prepare two segments");
            let mut completion = manager.observe_outbound(original);
            let mut active = original;
            if segment > 0 {
                manager.confirm_outbound_dispatch(original, true);
            }
            if segment == 2 {
                let proof = ResourceProof {
                    resource_hash: original,
                    proof: manager.outgoing.get(&original).expect("first sender").expected_proof,
                };
                let packets = manager.handle_packet(
                    &resource_packet(PacketContext::ResourceProof, &proof.encode(), *link.id()),
                    &mut link,
                );
                active = decrypt_advertisement(&link, &packets[0]).hash;
                assert_ne!(active, original);
                manager.drain_events();
                assert_eq!(completion.current(), None, "first segment is not logical completion");
            }
            match outcome {
                ResourceOutcome::Failed => manager.remove_link_state(*link.id()),
                ResourceOutcome::Cancelled => {
                    manager.cancel_outgoing(original, &link).expect("local cancellation");
                }
                ResourceOutcome::Rejected => {
                    manager.handle_packet(
                        &resource_packet(
                            PacketContext::ResourceReceiverCancel,
                            active.as_slice(),
                            *link.id(),
                        ),
                        &mut link,
                    );
                }
                ResourceOutcome::Complete => unreachable!(),
            }
            let events = manager.drain_events();
            assert_eq!(events.len(), 1, "one legacy terminal event");
            assert_eq!(events[0].hash, original);
            assert_eq!(completion.current(), Some(outcome));
            assert!(manager.outbound_completions.is_empty());
            assert!(manager.has_no_outbound_state());
            assert!(manager.outgoing_segment_chains.is_empty());
            // Repeated cleanup must not change a published terminal outcome.
            manager.remove_link_state(*link.id());
            assert!(manager.drain_events().is_empty());
            drop(manager);
            assert_eq!(completion.wait().await.expect("retained terminal result"), outcome);
        }
    }
}

#[test]
fn resource_cancellation_rejects_an_unrelated_link_without_changing_native_state() {
    for segment in [0, 1, 2] {
        let (mut manager, mut link) = manager_with_test_link();
        let (_, unrelated) = manager_with_test_link();
        assert_ne!(link.id(), unrelated.id());
        let (original, _) = manager
            .start_send(&link, vec![0x51; MAX_EFFICIENT_SIZE + 1], None)
            .expect("prepare two segments");
        let completion = manager.observe_outbound(original);
        if segment > 0 {
            manager.confirm_outbound_dispatch(original, true);
        }
        let mut active = original;
        if segment == 2 {
            let proof = ResourceProof {
                resource_hash: original,
                proof: manager.outgoing.get(&original).expect("first sender").expected_proof,
            };
            let packets = manager.handle_packet(
                &resource_packet(PacketContext::ResourceProof, &proof.encode(), *link.id()),
                &mut link,
            );
            active = decrypt_advertisement(&link, &packets[0]).hash;
            assert_ne!(active, original);
            manager.drain_events();
        }
        let before = (manager.pending_outgoing.len(), manager.outgoing.len());
        assert!(matches!(
            manager.cancel_outgoing(original, &unrelated),
            Err(RnsError::InvalidArgument)
        ));
        assert_eq!((manager.pending_outgoing.len(), manager.outgoing.len()), before);
        assert!(manager.outgoing_segment_chains.contains_key(&original));
        assert!(manager.drain_events().is_empty());
        assert_eq!(completion.current(), None, "mismatch must not publish cancellation");

        let packet = manager.cancel_outgoing(original, &link).expect("matching Link");
        if segment == 0 {
            assert!(packet.is_none(), "undispatched advertisement needs no cancel frame");
        } else {
            let packet = packet.expect("cancel active segment");
            assert_eq!(packet.destination, *link.id());
            assert_eq!(decrypt_link_packet(&link, &packet).data.as_slice(), active.as_slice());
        }
        let events = manager.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].hash, original);
        assert_eq!(events[0].link_id, *link.id());
        assert_eq!(completion.current(), Some(ResourceOutcome::Cancelled));
        assert!(manager.has_no_outbound_state());
        assert!(manager.outgoing_segment_chains.is_empty());
        assert!(manager.cancel_outgoing(original, &unrelated).expect("already absent").is_none());
    }
}

#[tokio::test]
async fn resource_completion_wait_survives_producer_removal_only_with_a_result() {
    let (mut manager, link) = manager_with_test_link();
    let (hash, _) = manager.start_send(&link, b"pending".to_vec(), None).expect("prepare");
    let mut completion = manager.observe_outbound(hash);
    // Cancelling the wait does not consume the subsequent result.
    assert!(tokio::time::timeout(Duration::from_millis(1), completion.wait()).await.is_err());
    manager.confirm_outbound_dispatch(hash, false);
    assert!(matches!(manager.drain_events()[0].kind, ResourceEventKind::OutboundFailed));
    drop(manager);
    for _ in 0..2 {
        assert_eq!(completion.wait().await.expect("retained failure"), ResourceOutcome::Failed);
    }

    let (mut manager, link) = manager_with_test_link();
    let (hash, _) = manager.start_send(&link, b"abandoned".to_vec(), None).expect("prepare");
    let mut completion = manager.observe_outbound(hash);
    drop(manager);
    assert_eq!(completion.current(), None);
    assert!(completion.wait().await.is_err(), "producer disappearance is not a network result");
}

#[test]
fn resource_completion_receiver_drop_preserves_legacy_lifecycle() {
    let (mut manager, link) = manager_with_test_link();
    let (hash, _) = manager.start_send(&link, b"unobserved".to_vec(), None).expect("prepare");
    assert!(manager.outbound_completions.is_empty(), "legacy sends allocate no observer");
    drop(manager.observe_outbound(hash));
    manager.confirm_outbound_dispatch(hash, true);
    assert!(!manager.has_no_outbound_state(), "dropping observation does not cancel a Resource");
    manager.remove_link_state(*link.id());
    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, ResourceEventKind::OutboundFailed));
    assert_eq!(events[0].hash, hash);
    assert!(manager.outbound_completions.is_empty());
}

#[tokio::test]
async fn resource_completion_requires_final_proof_of_all_segments() {
    let (mut sender_link, mut receiver_link) = resource_link_pair();
    let mut sender = ResourceManager::new();
    let mut receiver = ResourceManager::new();
    let payload: Vec<u8> = (0..MAX_EFFICIENT_SIZE + 37).map(|n| (n % 251) as u8).collect();
    let (original, advertisement) =
        sender.start_send(&sender_link, payload.clone(), None).expect("prepare");
    let mut completion = sender.observe_outbound(original);
    sender.confirm_outbound_dispatch(original, true);
    let mut to_receiver = vec![advertisement];
    let mut delivered = None;
    let mut proofs = 0;
    let mut terminal_events = 0;
    for _ in 0..100_000 {
        if to_receiver.is_empty() {
            break;
        }
        let mut to_sender = Vec::new();
        for packet in std::mem::take(&mut to_receiver) {
            let plain = decrypt_link_packet(&receiver_link, &packet);
            to_sender.extend(receiver.handle_packet(&plain, &mut receiver_link));
        }
        for event in receiver.drain_events() {
            if let ResourceEventKind::Complete(resource) = event.kind {
                delivered = Some(resource.data);
            }
        }
        for packet in to_sender {
            if packet.context == PacketContext::ResourceProof {
                proofs += 1;
                assert_eq!(completion.current(), None, "no success before final recipient proof");
            }
            let plain = decrypt_link_packet(&sender_link, &packet);
            to_receiver.extend(sender.handle_packet(&plain, &mut sender_link));
            for event in sender.drain_events() {
                if matches!(event.kind, ResourceEventKind::OutboundComplete) {
                    assert_eq!(event.hash, original);
                    terminal_events += 1;
                }
            }
        }
    }
    assert_eq!(proofs, 2);
    assert_eq!(terminal_events, 1);
    assert_eq!(delivered, Some(payload));
    assert_eq!(completion.current(), Some(ResourceOutcome::Complete));
    assert!(sender.outbound_completions.is_empty());
    drop(sender);
    assert_eq!(completion.wait().await.expect("retained proof result"), ResourceOutcome::Complete);
}
