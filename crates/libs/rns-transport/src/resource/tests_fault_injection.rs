/// A fragment outside the active receive window is not a valid completion for
/// the current request round. This is the deterministic reordering/duplicate
/// boundary from `RNS/Resource.py::receive_part`: delayed data must wait for a
/// later request window instead of moving the contiguous frontier forward.
#[test]
fn receiver_ignores_fragments_outside_the_current_window() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let link = Link::new(destination, tx);

    let random_hash = [0x31; RANDOM_HASH_SIZE];
    let parts: Vec<Vec<u8>> = (0..8)
        .map(|index| format!("resource-part-{index}").into_bytes())
        .collect();
    let mut hashmap = Vec::with_capacity(parts.len() * MAPHASH_LEN);
    for part in &parts {
        hashmap.extend_from_slice(&map_hash(part, &random_hash));
    }
    let resource_hash = Hash::new_from_slice(&[0x52; HASH_SIZE]);
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
    let mut receiver = ResourceReceiver::new(&advertisement, *link.id()).expect("receiver");
    receiver.window = 2;

    assert!(matches!(
        receiver.handle_part(&parts[3], &link),
        PartOutcome::NoMatch
    ));
    assert_eq!(receiver.received, 0, "future data must not enter the active round");
    assert_eq!(receiver.consecutive_completed_height, 0);

    assert!(matches!(
        receiver.handle_part(&parts[0], &link),
        PartOutcome::Incomplete
    ));
    assert_eq!(receiver.received, 1);

    // Once the first round is complete, a duplicate from that old round is
    // still outside the new search slice, while the next requested fragment
    // is admitted normally.
    receiver.parts[1] = Some(parts[1].clone());
    receiver.received += 1;
    receiver.consecutive_completed_height = 2;
    assert!(matches!(
        receiver.handle_part(&parts[0], &link),
        PartOutcome::NoMatch
    ));
    assert!(matches!(
        receiver.handle_part(&parts[2], &link),
        PartOutcome::Incomplete
    ));
    assert_eq!(receiver.received, 3);
}

fn resource_link_pair() -> (Link, Link) {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "resource-faults"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut sender = Link::new(destination, tx.clone());
    let request = sender.request();
    let mut receiver = Link::new_from_request(
        &request,
        signer.sign_key().clone(),
        destination,
        tx,
    )
    .expect("link request should parse");
    assert!(matches!(
        sender.handle_packet(&receiver.prove(), AddressHash::new_from_rand(OsRng)),
        LinkHandleResult::Activated
    ));
    (sender, receiver)
}

fn apply_reordering_loss_and_duplication(
    packets: &mut Vec<Packet>,
    reordered: &mut bool,
    dropped: &mut bool,
    duplicated: &mut bool,
) {
    let mut resource_positions: Vec<usize> = packets
        .iter()
        .enumerate()
        .filter_map(|(index, packet)| (packet.context == PacketContext::Resource).then_some(index))
        .collect();
    if resource_positions.is_empty() {
        return;
    }

    if !*reordered && resource_positions.len() >= 2 {
        packets.swap(resource_positions[0], resource_positions[1]);
        *reordered = true;
    }
    resource_positions = packets
        .iter()
        .enumerate()
        .filter_map(|(index, packet)| (packet.context == PacketContext::Resource).then_some(index))
        .collect();
    if !*dropped {
        packets.remove(resource_positions[0]);
        *dropped = true;
    }
    resource_positions = packets
        .iter()
        .enumerate()
        .filter_map(|(index, packet)| (packet.context == PacketContext::Resource).then_some(index))
        .collect();
    if !*duplicated {
        let index = resource_positions[0];
        packets.insert(index, packets[index].clone());
        *duplicated = true;
    }
}

/// The manager must recover a real transfer after one round is reordered, one
/// fragment is dropped, and another is duplicated. This keeps the fault model
/// deterministic while exercising the production Resource request/proof
/// state machine rather than only testing individual receiver methods.
#[test]
fn resource_transfer_recovers_from_loss_duplication_and_reordering() {
    let (mut sender_link, mut receiver_link) = resource_link_pair();
    let mut sender = ResourceManager::new_with_config(Duration::from_secs(1), 8);
    let mut receiver = ResourceManager::new_with_config(Duration::from_secs(1), 8);
    let payload: Vec<u8> = (0..(PACKET_MDU * 32))
        .map(|index| ((index * 37 + 11) % 251) as u8)
        .collect();
    let (resource_hash, advertisement) =
        sender.start_send(&sender_link, payload.clone(), None).expect("start resource");
    sender.confirm_outbound_dispatch(resource_hash, true);

    let mut to_receiver = vec![advertisement];
    let mut complete = None;
    let mut reordered = false;
    let mut dropped = false;
    let mut duplicated = false;

    for step in 0..256 {
        let mut to_sender = Vec::new();
        for packet in std::mem::take(&mut to_receiver) {
            let plain = decrypt_link_packet(&receiver_link, &packet);
            to_sender.extend(receiver.handle_packet(&plain, &mut receiver_link));
        }

        // The first advertisement already opened a request round. Starting
        // with the following iteration, advance a deterministic clock far
        // enough to make any dropped in-flight fragment eligible for retry.
        if step > 0 {
            for (link_id, request) in receiver.retry_requests(
                Instant::now() + Duration::from_secs(30_u64 + step as u64),
            ) {
                let packet = build_link_packet(
                    &receiver_link,
                    PacketType::Data,
                    PacketContext::ResourceRequest,
                    &request.encode(),
                )
                .expect("retry request packet");
                assert_eq!(link_id, *receiver_link.id());
                to_sender.push(packet);
            }
        }

        let mut next_to_receiver = Vec::new();
        for packet in to_sender {
            let plain = decrypt_link_packet(&sender_link, &packet);
            next_to_receiver.extend(sender.handle_packet(&plain, &mut sender_link));
        }
        apply_reordering_loss_and_duplication(
            &mut next_to_receiver,
            &mut reordered,
            &mut dropped,
            &mut duplicated,
        );
        to_receiver = next_to_receiver;

        for event in receiver.drain_events() {
            if let ResourceEventKind::Complete(resource) = event.kind {
                complete = Some(resource.data);
            }
        }
        sender.drain_events();
        if complete.is_some() && sender.has_no_outbound_state() {
            break;
        }
    }

    assert!(reordered, "the first request round must have been reordered");
    assert!(dropped, "the first request round must have dropped a fragment");
    assert!(duplicated, "the first request round must have duplicated a fragment");
    assert_eq!(complete.as_deref(), Some(payload.as_slice()));
    assert!(sender.has_no_outbound_state());
}

/// A split sender must release its reader/buffer tail regardless of which
/// segment the peer cancels. Testing every segment catches the easy-to-miss
/// distinction between the active segment hash and the original chain hash.
#[test]
fn split_resource_cancellation_clears_state_at_each_segment() {
    for target_segment in 1..=3u32 {
        let (mut sender_link, _) = resource_link_pair();
        let mut sender = ResourceManager::new_with_config(Duration::from_secs(1), 2);
        let data: Vec<u8> = (0..(MAX_EFFICIENT_SIZE * 2 + 257))
            .map(|index| ((index * 19 + 7) % 251) as u8)
            .collect();
        let (original_hash, _) =
            sender.start_send(&sender_link, data, None).expect("start split resource");
        sender.confirm_outbound_dispatch(original_hash, true);
        let mut active_hash = original_hash;

        for expected_segment in 2..=target_segment {
            let expected_proof = sender
                .outgoing
                .get(&active_hash)
                .expect("active segment")
                .expected_proof;
            let proof = ResourceProof { resource_hash: active_hash, proof: expected_proof };
            let packets = sender.handle_packet(
                &resource_packet(
                    PacketContext::ResourceProof,
                    &proof.encode(),
                    *sender_link.id(),
                ),
                &mut sender_link,
            );
            let advertisement = packets
                .iter()
                .find(|packet| packet.context == PacketContext::ResourceAdvrtisement)
                .expect("next segment advertisement");
            let decoded = decrypt_advertisement(&sender_link, advertisement);
            assert_eq!(decoded.segment_index, expected_segment);
            active_hash = decoded.hash;
        }

        sender.handle_packet(
            &resource_packet(
                PacketContext::ResourceReceiverCancel,
                active_hash.as_slice(),
                *sender_link.id(),
            ),
            &mut sender_link,
        );

        assert!(sender.has_no_outbound_state(), "cancelled segment {target_segment} leaked state");
        assert!(sender.outgoing_segment_chains.is_empty());
        let events = sender.drain_events();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.hash == original_hash
                    && matches!(event.kind, ResourceEventKind::OutboundRejected))
                .count(),
            1,
            "cancelled segment {target_segment} must emit one terminal event: {events:?}"
        );
    }
}
