#[test]
fn resource_receiver_uses_advertised_hashmap_stride_for_updates() {
    let random_hash = [3u8; RANDOM_HASH_SIZE];
    let parts = [
        b"part-00".as_slice(),
        b"part-01".as_slice(),
        b"part-02".as_slice(),
        b"part-03".as_slice(),
    ];
    let map_hashes: Vec<[u8; MAPHASH_LEN]> =
        parts.iter().map(|part| map_hash(part, &random_hash)).collect();
    let mut first_segment = Vec::with_capacity(MAPHASH_LEN * 2);
    first_segment.extend_from_slice(&map_hashes[0]);
    first_segment.extend_from_slice(&map_hashes[1]);
    let mut second_segment = Vec::with_capacity(MAPHASH_LEN * 2);
    second_segment.extend_from_slice(&map_hashes[2]);
    second_segment.extend_from_slice(&map_hashes[3]);
    let adv = ResourceAdvertisement {
        transfer_size: parts.iter().map(|part| part.len() as u64).sum(),
        data_size: parts.iter().map(|part| part.len() as u64).sum(),
        parts: parts.len() as u32,
        hash: Hash::new_from_slice(&[9u8; 32]),
        random_hash,
        original_hash: Hash::new_from_slice(&[9u8; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: first_segment,
    };

    let mut receiver =
        ResourceReceiver::new(&adv, AddressHash::new_from_slice(&[4u8; ADDRESS_HASH_SIZE]))
            .expect("receiver accepts smaller advertised hashmap segment");
    assert_eq!(receiver.hashmap_segment_len, 2);

    receiver.handle_hash_update(&ResourceHashUpdate {
        resource_hash: adv.hash,
        segment: 1,
        hashmap: second_segment,
    });

    assert_eq!(receiver.hashmap, map_hashes.into_iter().map(Some).collect::<Vec<_>>());
}

/// Builds a receiver for a resource whose hashmap needs several
/// segments, with only segment zero known — the state every large
/// transfer starts in.
fn multi_segment_receiver(
    total_parts: usize,
    segment_len: usize,
) -> (ResourceReceiver, Vec<[u8; MAPHASH_LEN]>) {
    let random_hash = [7u8; RANDOM_HASH_SIZE];
    let bodies: Vec<Vec<u8>> =
        (0..total_parts).map(|i| format!("part-{i:05}").into_bytes()).collect();
    let map_hashes: Vec<[u8; MAPHASH_LEN]> =
        bodies.iter().map(|body| map_hash(body, &random_hash)).collect();
    let mut first_segment = Vec::with_capacity(MAPHASH_LEN * segment_len);
    for hash in map_hashes.iter().take(segment_len) {
        first_segment.extend_from_slice(hash);
    }
    let adv = ResourceAdvertisement {
        transfer_size: bodies.iter().map(|body| body.len() as u64).sum(),
        data_size: bodies.iter().map(|body| body.len() as u64).sum(),
        parts: total_parts as u32,
        hash: Hash::new_from_slice(&[11u8; 32]),
        random_hash,
        original_hash: Hash::new_from_slice(&[11u8; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: first_segment,
    };
    let receiver =
        ResourceReceiver::new(&adv, AddressHash::new_from_slice(&[5u8; ADDRESS_HASH_SIZE]))
            .expect("advertisement with a partial hashmap is valid");
    (receiver, map_hashes)
}

/// Exhaustion means "the fragments I want next are unmapped", not "some
/// fragment somewhere is unmapped".
///
/// The difference is not cosmetic. Signalling exhaustion makes the
/// reference sender advance `receiver_min_consecutive_height` by a whole
/// hashmap segment (`RNS/Resource.py`), and it only serves fragments
/// inside `parts[that .. that + COLLISION_GUARD_SIZE]`. Signalling it on
/// every request walks that window off the end of the fragments actually
/// being requested, and the sender then drops them silently.
///
/// Measured against a real NomadNet node before this fix: 8 fragments of
/// 2260 arrived, followed by 266 hashmap-update packets and no further
/// data until the transfer timed out.
#[test]
fn a_mapped_request_window_does_not_report_hashmap_exhaustion() {
    let segment_len = 74;
    let (mut receiver, _) = multi_segment_receiver(600, segment_len);

    let request = receiver.build_request(
        Instant::now(),
        Duration::from_millis(50),
        TEST_ARRIVAL_INTERVAL,
        RequestTrigger::Immediate,
    );

    assert!(
        !request.hashmap_exhausted,
        "the first {WINDOW} fragments are mapped by segment zero — nothing to ask the sender for"
    );
    assert_eq!(
        request.requested_hashes.len(),
        WINDOW,
        "the window should be full of real fragment requests"
    );
}

/// Once the map *is* exhausted, exactly one request goes out and the
/// receiver waits. Without the gate it re-asks at link RTT, and each
/// re-ask moves the reference sender's serving window forward again.
#[test]
fn an_outstanding_hashmap_update_suppresses_further_requests() {
    let segment_len = 2;
    let (mut receiver, map_hashes) = multi_segment_receiver(8, segment_len);
    let now = Instant::now();

    // Nothing received yet, so the window is [0, WINDOW) and segment
    // zero maps only its first two slots — the state a large transfer
    // is in from its very first request.
    let first = receiver.build_request(
        now,
        Duration::from_millis(50),
        TEST_ARRIVAL_INTERVAL,
        RequestTrigger::Immediate,
    );
    assert!(
        first.hashmap_exhausted,
        "the next fragment is unmapped, so the map has to be asked for"
    );
    assert_eq!(
        first.last_map_hash,
        Some(map_hashes[segment_len - 1]),
        "the sender matches this against its own parts and cancels the transfer if it is not a segment boundary"
    );

    let second = receiver.build_request(
        now,
        Duration::from_millis(50),
        TEST_ARRIVAL_INTERVAL,
        RequestTrigger::Immediate,
    );
    assert!(
        !second.hashmap_exhausted,
        "asking twice for the same segment is what walks the sender's window off"
    );
    assert!(second.requested_hashes.is_empty(), "…and there is nothing else to ask for either");

    // The update lands: the receiver resumes immediately.
    let mut segment_bytes = Vec::new();
    for hash in map_hashes.iter().skip(segment_len).take(segment_len) {
        segment_bytes.extend_from_slice(hash);
    }
    receiver.handle_hash_update(&ResourceHashUpdate {
        resource_hash: receiver.resource_hash,
        segment: 1,
        hashmap: segment_bytes,
    });

    let third = receiver.build_request(
        now,
        Duration::from_millis(50),
        TEST_ARRIVAL_INTERVAL,
        RequestTrigger::Immediate,
    );
    assert_eq!(
        third.requested_hashes,
        map_hashes[segment_len..segment_len * 2].to_vec(),
        "the newly mapped fragments are requested straight away"
    );
    assert!(!third.hashmap_exhausted, "and the window they fill is mapped again");
}

/// The window opens as rounds succeed, instead of sitting at four for
/// the whole transfer.
///
/// Four fragments per round is the reference's *starting* window; it
/// grows toward `WINDOW_MAX_SLOW` and, on a link measured fast enough,
/// to `WINDOW_MAX_FAST`. Holding it at four caps throughput at four
/// fragments per round trip no matter how good the link is — on a
/// 2260-fragment resource that is 565 sequential round trips.
#[test]
fn the_request_window_opens_as_rounds_succeed() {
    let (mut receiver, map_hashes) = multi_segment_receiver(600, 600);
    let mut now = Instant::now();
    let rtt = Duration::from_millis(50);

    assert_eq!(
        receiver
            .build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate)
            .requested_hashes
            .len(),
        WINDOW,
        "the first round is the starting window"
    );

    // Complete round after round, delivering exactly what was asked for.
    let mut delivered = 0usize;
    let mut sizes = Vec::new();
    for _ in 0..6 {
        while delivered < receiver.consecutive_completed_height + receiver.window {
            receiver.parts[delivered] = Some(b"body".to_vec());
            receiver.received += 1;
            receiver.in_flight_set.remove(&delivered);
            delivered += 1;
        }
        receiver.consecutive_completed_height = delivered;
        receiver.note_round_complete(now);
        now += rtt;
        sizes.push(
            receiver
                .build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate)
                .requested_hashes
                .len(),
        );
    }

    assert!(
        sizes.windows(2).all(|pair| pair[1] >= pair[0]),
        "a window that shrinks on success would be worse than a fixed one: {sizes:?}"
    );
    assert!(
        sizes.last().unwrap() > &WINDOW,
        "six clean rounds must buy more than the starting window: {sizes:?}"
    );
    assert!(
        sizes.iter().all(|size| *size <= WINDOW_MAX_SLOW),
        "…but not past the slow ceiling until the link measures fast: {sizes:?}"
    );
    let _ = map_hashes;
}

/// Growth has to survive a pipeline that never empties.
///
/// This receiver refills the window on every part rather than draining
/// it, so a "nothing in flight" trigger starves itself: the bigger the
/// window, the rarer a full drain, and growth stops exactly where it
/// matters. Measured against a real node that ceiling was 42 against an
/// allowed 75 — with 97% of rounds showing the window completely full,
/// so the window, not the link, was the limit.
#[test]
fn the_window_keeps_growing_while_the_pipeline_stays_full() {
    let (mut receiver, _) = multi_segment_receiver(4000, 4000);
    let mut now = Instant::now();

    // Deliver fragments steadily, never letting the window drain.
    for _ in 0..3000 {
        now += Duration::from_millis(1);
        receiver.received_bytes += 464;
        receiver.note_fragment_received(now);
    }

    assert_eq!(
        receiver.window, WINDOW_MAX_FAST,
        "a steadily-delivering link must reach the ceiling, not stall below it"
    );
}

/// …and closes again when fragments go missing, once per round rather
/// than once per fragment.
///
/// A window of four that times out wholesale is one failed round. Four
/// separate shrinks would collapse straight to the floor and discard
/// everything the link had proven.
#[test]
fn a_round_of_losses_narrows_the_window_once() {
    let (mut receiver, _) = multi_segment_receiver(600, 600);
    let now = Instant::now();
    let rtt = Duration::from_millis(50);

    // Open the window up first, so there is room to shrink.
    for _ in 0..5 {
        receiver.note_round_complete(now);
    }
    let opened = receiver.window;
    assert!(opened > WINDOW_MIN, "precondition: the window has room to close");

    receiver.build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);
    // Every fragment just requested times out together.
    let much_later = now + rtt * 100;
    receiver.build_request(much_later, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate);

    assert_eq!(
        receiver.window,
        opened - 1,
        "one failed round, one step back — not one per fragment"
    );
    assert!(receiver.window >= WINDOW_MIN);
}

/// The fast ceiling exists, is reached only by sustained measurement,
/// and stops exactly where the sender's serving window assumes it will.
#[test]
fn a_sustained_fast_link_unlocks_the_fast_ceiling_and_no_further() {
    let (mut receiver, _) = multi_segment_receiver(600, 600);
    let mut now = Instant::now();

    // Rounds that move plenty of data in very little time.
    for _ in 0..FAST_RATE_THRESHOLD {
        receiver.received_bytes += 100_000;
        now += Duration::from_millis(10);
        receiver.note_round_complete(now);
    }
    assert_eq!(
        receiver.window_max, WINDOW_MAX_FAST,
        "sustained fast rounds unlock the fast ceiling"
    );

    for _ in 0..500 {
        now += Duration::from_millis(10);
        receiver.received_bytes += 100_000;
        receiver.note_round_complete(now);
    }
    assert_eq!(
        receiver.window, WINDOW_MAX_FAST,
        "the window stops at the sender's own WINDOW_MAX — asking past it means asking for fragments \
         a reference sender has already stopped serving"
    );
}

/// A lost hashmap update must not park the transfer forever.
///
/// It would, and silently: `retry_count` only advances when a request is
/// actually sent, so a permanently-gated receiver is never even declared
/// failed. This is issue #369's failure mode exactly.
#[test]
fn a_lost_hashmap_update_is_re_requested_rather_than_hanging() {
    let segment_len = 2;
    let (mut receiver, _) = multi_segment_receiver(8, segment_len);
    let now = Instant::now();
    let rtt = Duration::from_millis(50);

    assert!(
        receiver
            .build_request(now, rtt, TEST_ARRIVAL_INTERVAL, RequestTrigger::Immediate)
            .hashmap_exhausted
    );
    receiver.mark_request();

    let still_waiting = receiver.build_request(
        now + Duration::from_millis(10),
        rtt,
        TEST_ARRIVAL_INTERVAL,
        RequestTrigger::Immediate,
    );
    assert!(!still_waiting.hashmap_exhausted, "a reply is still plausibly in flight");

    let gave_up = receiver.build_request(
        now + hashmap_update_wait(rtt) + Duration::from_secs(1),
        rtt,
        TEST_ARRIVAL_INTERVAL,
        RequestTrigger::Immediate,
    );
    assert!(
        gave_up.hashmap_exhausted,
        "the update never came — ask again rather than wait forever"
    );
}

#[test]
fn resource_completion_preserves_request_response_metadata() {
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

    let data = b"file-response";
    let random_hash = [0x66; RANDOM_HASH_SIZE];
    let mut part = Vec::with_capacity(RANDOM_HASH_SIZE + data.len());
    part.extend_from_slice(&random_hash);
    part.extend_from_slice(data);
    let resource_hash = Hash::new_from_slice(&[data.as_slice(), &random_hash].concat());
    let request_id = vec![0x44; ADDRESS_HASH_SIZE];
    let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
    hashmap.extend_from_slice(&map_hash(&part, &random_hash));
    let adv = ResourceAdvertisement {
        transfer_size: part.len() as u64,
        data_size: data.len() as u64,
        parts: 1,
        hash: resource_hash,
        random_hash,
        original_hash: resource_hash,
        segment_index: 1,
        total_segments: 1,
        request_id: Some(ByteBuf::from(request_id.clone())),
        flags: FLAG_RESPONSE,
        hashmap,
    };
    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *link.id(),
    );

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let request_packets = manager.handle_packet(&packet, &mut link);
    assert_eq!(request_packets.len(), 1);
    let part_packet = resource_packet(PacketContext::Resource, &part, *link.id());
    let proof_packets = manager.handle_packet(&part_packet, &mut link);
    assert_eq!(proof_packets.len(), 1);

    let events = manager.drain_events();
    let complete = events
        .into_iter()
        .find_map(|event| match event.kind {
            ResourceEventKind::Complete(complete) => Some(complete),
            _ => None,
        })
        .expect("resource complete event");
    assert_eq!(complete.data, data);
    assert_eq!(complete.request_id.as_deref(), Some(request_id.as_slice()));
    assert!(complete.is_response);
    assert!(!complete.is_request);
}

#[test]
fn resource_manager_removes_failed_receiver_without_followup_request() {
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

    let part = b"not-bzip";
    let random_hash = [5u8; RANDOM_HASH_SIZE];
    let resource_hash = Hash::new_from_slice(&[8u8; 32]);
    let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
    hashmap.extend_from_slice(&map_hash(part, &random_hash));
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
        flags: FLAG_COMPRESSED,
        hashmap,
    };

    let adv_packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *link.id(),
    );

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let first = manager.handle_packet(&adv_packet, &mut link);
    assert_eq!(first.len(), 1);
    assert_eq!(manager.incoming.len(), 1);

    let part_packet = resource_packet(PacketContext::Resource, part, *link.id());
    let responses = manager.handle_packet(&part_packet, &mut link);
    assert!(responses.is_empty());
    assert!(manager.incoming.is_empty());
    let events = manager.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].hash, resource_hash);
    assert_eq!(events[0].link_id, *link.id());
    let ResourceEventKind::InboundFailed(failure) = &events[0].kind else {
        panic!("expected inbound failure event");
    };
    assert_eq!(failure.reason, "decompress_failed");
    assert_eq!(failure.progress.received_parts, 1);
}

#[test]
fn resource_receiver_reports_failure_reason() {
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

    let part = b"not-bzip";
    let random_hash = [5u8; RANDOM_HASH_SIZE];
    let mut hashmap = Vec::with_capacity(MAPHASH_LEN);
    hashmap.extend_from_slice(&map_hash(part, &random_hash));
    let adv = ResourceAdvertisement {
        transfer_size: part.len() as u64,
        data_size: part.len() as u64,
        parts: 1,
        hash: Hash::new_from_slice(&[8u8; 32]),
        random_hash,
        original_hash: Hash::new_from_slice(&[8u8; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: FLAG_COMPRESSED,
        hashmap,
    };
    let mut receiver = ResourceReceiver::new(&adv, *link.id()).expect("resource receiver");

    assert!(matches!(receiver.handle_part(part, &link), PartOutcome::Failed("decompress_failed")));
}

// Regression tests for issue #514: the manager must reject
// advertisements exceeding MAX_INBOUND_RESOURCE_TRANSFER_SIZE or
// MAX_INBOUND_RESOURCE_PARTS before any receiver (and its
// part-tracking allocations) is created.
fn advertisement_with(transfer_size: u64, data_size: u64, parts: u32) -> ResourceAdvertisement {
    ResourceAdvertisement {
        transfer_size,
        data_size,
        parts,
        hash: Hash::new_from_slice(&[3u8; 32]),
        random_hash: [0u8; RANDOM_HASH_SIZE],
        original_hash: Hash::new_from_slice(&[3u8; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: vec![0u8; MAPHASH_LEN],
    }
}

fn manager_with_test_link() -> (ResourceManager, Link) {
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
    (ResourceManager::new_with_config(Duration::from_secs(1), 1), link)
}

#[test]
fn resource_manager_rejects_advertisement_over_transfer_size_limit() {
    let (mut manager, mut link) = manager_with_test_link();
    // A multi-gigabyte advertised transfer must never reach receiver
    // creation; a transfer exactly at the cap still must.
    let oversized = advertisement_with(MAX_INBOUND_RESOURCE_TRANSFER_SIZE + 1, 1, 1);
    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &oversized.pack().expect("advertisement"),
        *link.id(),
    );

    let responses = manager.handle_packet(&packet, &mut link);
    assert!(responses.is_empty(), "rejected advertisement must not produce a request");
    assert!(manager.incoming.is_empty(), "rejected advertisement must not create a receiver");

    let at_limit = advertisement_with(MAX_INBOUND_RESOURCE_TRANSFER_SIZE, 1, 1);
    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &at_limit.pack().expect("advertisement"),
        *link.id(),
    );
    let responses = manager.handle_packet(&packet, &mut link);
    assert!(!manager.incoming.is_empty(), "transfer at the limit is still accepted");
    assert!(!responses.is_empty(), "accepted advertisement requests parts");
}

#[test]
fn resource_manager_rejects_advertisement_over_parts_limit() {
    let (mut manager, mut link) = manager_with_test_link();
    // An excessive part count must never reach receiver creation; a
    // count exactly at the cap still must.
    let oversized = advertisement_with(1, 1, (MAX_INBOUND_RESOURCE_PARTS + 1) as u32);
    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &oversized.pack().expect("advertisement"),
        *link.id(),
    );

    let responses = manager.handle_packet(&packet, &mut link);
    assert!(responses.is_empty(), "rejected advertisement must not produce a request");
    assert!(manager.incoming.is_empty(), "rejected advertisement must not create a receiver");

    let at_limit = advertisement_with(1, 1, 1);
    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &at_limit.pack().expect("advertisement"),
        *link.id(),
    );
    let responses = manager.handle_packet(&packet, &mut link);
    assert!(!manager.incoming.is_empty(), "valid advertisement is still accepted");
    assert!(!responses.is_empty());
}

#[test]
fn resource_receiver_rejects_unreasonable_advertised_parts() {
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

    let adv = ResourceAdvertisement {
        transfer_size: 1,
        data_size: 1,
        parts: 2,
        hash: Hash::new_from_slice(&[3u8; 32]),
        random_hash: [0u8; RANDOM_HASH_SIZE],
        original_hash: Hash::new_from_slice(&[3u8; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: vec![0u8; MAPHASH_LEN * 2],
    };

    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *link.id(),
    );

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let responses = manager.handle_packet(&packet, &mut link);

    assert!(responses.is_empty());
    assert!(manager.incoming.is_empty());
}

#[test]
fn resource_receiver_accepts_sender_with_smaller_effective_sdu() {
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

    let advertised_parts = 16;
    let adv = ResourceAdvertisement {
        transfer_size: PACKET_MDU as u64 + 1,
        data_size: PACKET_MDU as u64 + 1,
        parts: advertised_parts,
        hash: Hash::new_from_slice(&[4u8; 32]),
        random_hash: [0u8; RANDOM_HASH_SIZE],
        original_hash: Hash::new_from_slice(&[4u8; 32]),
        segment_index: 1,
        total_segments: 1,
        request_id: None,
        flags: 0,
        hashmap: vec![0u8; MAPHASH_LEN * advertised_parts as usize],
    };

    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &adv.pack().expect("advertisement"),
        *link.id(),
    );

    let mut manager = ResourceManager::new_with_config(Duration::from_secs(1), 1);
    let responses = manager.handle_packet(&packet, &mut link);

    assert_eq!(responses.len(), 1);
    assert_eq!(manager.incoming.len(), 1);
}

#[test]
fn resource_receiver_bounds_part_count_by_transfer_size_and_global_cap() {
    assert_eq!(max_advertised_parts(1, PACKET_MDU).expect("one byte transfer"), 1);
    assert_eq!(
        max_advertised_parts(PACKET_MDU as u64, PACKET_MDU).expect("one packet transfer"),
        PACKET_MDU as u64
    );
    assert_eq!(
        max_advertised_parts(PACKET_MDU as u64 + 1, PACKET_MDU).expect("larger transfer"),
        PACKET_MDU as u64 + 1
    );
    assert!(max_advertised_parts(0, PACKET_MDU).is_err());
    assert!(max_advertised_parts(MAX_INBOUND_RESOURCE_TRANSFER_SIZE + 1, PACKET_MDU).is_err());
    assert_eq!(
        max_advertised_parts(MAX_INBOUND_RESOURCE_TRANSFER_SIZE, PACKET_MDU)
            .expect("maximum transfer"),
        MAX_INBOUND_RESOURCE_PARTS
    );
}
