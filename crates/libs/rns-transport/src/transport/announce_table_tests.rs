use super::*;
use crate::packet::PacketContext;
use rand_core::OsRng;
use std::thread;
use std::time::Duration as StdDuration;

#[test]
fn announce_entries_use_random_window_and_grace_retry() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, ..Packet::default() };

    let before_insert = Instant::now();
    table.add(&packet, destination, received_from);
    let entry = table.map.get(&destination).expect("announce entry inserted");
    let initial_delay = entry
        .timeout
        .checked_duration_since(before_insert)
        .expect("retry timeout is after insertion");
    assert!(
        initial_delay <= PATHFINDER_RETRY_WINDOW,
        "initial retry window should stay inside python's 0.5s jitter window"
    );
    assert_eq!(entry.retries, 0);

    table.map.get_mut(&destination).unwrap().timeout = Instant::now() - Duration::from_millis(1);

    let messages = table.drain_retransmissions(&transport_id);
    assert_eq!(messages.len(), 1, "first local rebroadcast should fire once");
    let entry = table.map.get(&destination).expect("entry stays live for grace retry");
    assert_eq!(entry.retries, 1);

    table.map.get_mut(&destination).unwrap().timeout = Instant::now() - Duration::from_millis(1);
    let messages = table.drain_retransmissions(&transport_id);
    assert_eq!(messages.len(), 1, "python keeps one extra grace retry");
    assert!(!table.map.contains_key(&destination));
    assert!(table.drain_retransmissions(&transport_id).is_empty());
}

#[test]
fn path_response_entries_use_shorter_window_without_later_broadcast() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let to_iface = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, context: PacketContext::None, ..Packet::default() };

    table.add(&packet, destination, received_from);
    assert!(table.add_response(destination, to_iface, 3));
    assert!(
        table.map.contains_key(&destination),
        "live announce entry must stay available for later remote path requests"
    );
    assert!(table.drain_retransmissions(&transport_id).is_empty());
    assert_eq!(table.responses.len(), 1);
    let response = table.responses.get(&destination).expect("response entry inserted");
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    let response_delay = response.timeout.checked_duration_since(Instant::now()).unwrap_or_default();
    assert!(
        response_delay <= PATH_RESPONSE_GRACE,
        "path responses should stay on the shorter direct-response grace window"
    );

    thread::park_timeout(StdDuration::from_millis(450));

    let messages = table.drain_retransmissions(&transport_id);
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
    assert_eq!(messages[0].packet.context, PacketContext::PathResponse);
    assert!(table.responses.is_empty());
    assert!(table.map.contains_key(&destination));
    assert!(table.add_response(destination, to_iface, 4));

    thread::park_timeout(StdDuration::from_millis(450));

    let messages = table.drain_retransmissions(&transport_id);
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
    assert_eq!(messages[0].packet.header.hops, 4);
    assert_eq!(messages[0].packet.context, PacketContext::PathResponse);
    assert!(table.responses.is_empty());
    assert!(table.map.contains_key(&destination));
}

#[test]
fn due_path_response_preempts_same_destination_ordinary_announce_once() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let to_iface = AddressHash::new_from_rand(OsRng);
    let mut packet = Packet { destination, context: PacketContext::None, ..Packet::default() };
    packet.header.hops = 2;

    table.add(&packet, destination, received_from);
    assert!(table.add_response(destination, to_iface, 3));
    table.map.get_mut(&destination).expect("ordinary entry").timeout =
        Instant::now() - Duration::from_millis(1);
    table.responses.get_mut(&destination).expect("response entry").timeout =
        Instant::now() - Duration::from_millis(1);

    let first = table.drain_retransmissions(&transport_id);
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
    assert_eq!(first[0].packet.destination, destination);
    assert_eq!(first[0].packet.context, PacketContext::PathResponse);
    assert_eq!(first[0].packet.header.hops, 3);
    assert!(
        table.map.contains_key(&destination),
        "same-destination ordinary announce must remain queued while response drains"
    );
    assert!(
        table.responses.is_empty(),
        "completed path response must not keep suppressing the ordinary announce"
    );

    let second = table.drain_retransmissions(&transport_id);
    assert_eq!(second.len(), 1);
    assert!(
        matches!(second[0].tx_type, TxMessageType::Broadcast(Some(iface)) if iface == received_from)
    );
    assert_eq!(second[0].packet.destination, destination);
    assert_eq!(second[0].packet.context, PacketContext::None);
    assert_eq!(second[0].packet.header.hops, 2);
}

#[test]
fn path_response_entries_can_apply_extra_roaming_grace() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let to_iface = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, context: PacketContext::None, ..Packet::default() };

    table.add(&packet, destination, received_from);
    let before_response = Instant::now();
    assert!(table.add_response_with_extra_grace(
        destination,
        to_iface,
        3,
        PATH_RESPONSE_ROAMING_GRACE
    ));

    let response = table.responses.get(&destination).expect("response entry inserted");
    let response_delay = response.timeout.checked_duration_since(before_response).unwrap_or_default();
    assert!(
        response_delay >= PATH_RESPONSE_GRACE + PATH_RESPONSE_ROAMING_GRACE,
        "roaming path responses should include Python's extra roaming grace"
    );
    assert!(
        response_delay <= PATH_RESPONSE_GRACE + PATH_RESPONSE_ROAMING_GRACE + Duration::from_millis(250),
        "roaming grace should not add extra delay beyond scheduling jitter"
    );
    assert!(
        table.drain_retransmissions(&AddressHash::new_from_rand(OsRng)).is_empty(),
        "roaming path response must not drain before its delayed timeout"
    );

    table.responses.get_mut(&destination).expect("response entry").timeout =
        Instant::now() - Duration::from_millis(1);
    let messages = table.drain_retransmissions(&AddressHash::new_from_rand(OsRng));
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
}

#[test]
fn cached_path_response_entries_stamp_response_without_shadowing_cache() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let to_iface = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, context: PacketContext::None, ..Packet::default() };

    table.add(&packet, destination, received_from);
    let cached = table.map.remove(&destination).expect("announce entry inserted");
    table.cache.insert(destination, cached);

    assert!(table.add_response(destination, to_iface, 5));
    let response = table.responses.get(&destination).expect("cached response entry inserted");
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    assert_eq!(
        table
            .packet_for_destination(&destination)
            .expect("cached announce remains available")
            .context,
        PacketContext::None
    );

    thread::park_timeout(StdDuration::from_millis(450));

    let messages = table.drain_retransmissions(&transport_id);
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0].tx_type, TxMessageType::Direct(iface) if iface == to_iface));
    assert_eq!(messages[0].packet.context, PacketContext::PathResponse);
    assert_eq!(messages[0].packet.header.hops, 5);
    assert!(table.responses.is_empty());
}

#[test]
fn restored_cached_announces_do_not_rebroadcast_but_can_answer_path_requests() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let to_iface = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, context: PacketContext::None, ..Packet::default() };

    table.add_cached(&packet, destination, received_from);
    assert!(table.map.is_empty());
    assert!(table.drain_retransmissions(&transport_id).is_empty());
    assert_eq!(
        table
            .packet_for_destination(&destination)
            .expect("restored announce is lookup material")
            .context,
        PacketContext::None
    );

    assert!(table.add_response(destination, to_iface, 2));
    let response = table.responses.get(&destination).expect("cached response entry inserted");
    assert_eq!(response.response_to_iface, Some(to_iface));
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    assert_eq!(response.hops, 2);
}

#[test]
fn passed_on_rebroadcast_completes_pending_ordinary_announce() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, ..Packet::default() };

    table.add(&packet, destination, received_from);
    table.map.get_mut(&destination).expect("entry").timeout =
        Instant::now() - Duration::from_millis(1);
    assert_eq!(table.drain_retransmissions(&transport_id).len(), 1);
    let entry = table.map.get(&destination).expect("entry after first rebroadcast");
    assert_eq!(entry.retries, 1);
    let observed_hops = entry.hops + 1;

    assert!(
        table.observe_passed_rebroadcast(&destination, observed_hops),
        "Python removes a pending announce when a rebroadcast has been passed onward"
    );
    assert!(!table.map.contains_key(&destination));
    assert!(
        table.cached_packet_for_destination(&destination).is_some(),
        "completed announce material should remain available for known-path responses"
    );
    assert!(table.drain_retransmissions(&transport_id).is_empty());
}

#[test]
fn passed_on_rebroadcast_does_not_complete_before_local_retry() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, ..Packet::default() };

    table.add(&packet, destination, received_from);
    let observed_hops = table.map.get(&destination).expect("entry").hops + 2;

    assert!(
        !table.observe_passed_rebroadcast(&destination, observed_hops),
        "Python only completes passed-on announces after at least one local retry"
    );
    assert!(table.map.contains_key(&destination));
}

#[test]
fn passed_on_rebroadcast_requires_next_hop_count() {
    let mut table = AnnounceTable::new(16, 1);
    let destination = AddressHash::new_from_rand(OsRng);
    let received_from = AddressHash::new_from_rand(OsRng);
    let transport_id = AddressHash::new_from_rand(OsRng);
    let packet = Packet { destination, ..Packet::default() };

    table.add(&packet, destination, received_from);
    table.map.get_mut(&destination).expect("entry").timeout =
        Instant::now() - Duration::from_millis(1);
    assert_eq!(table.drain_retransmissions(&transport_id).len(), 1);
    let observed_hops = table.map.get(&destination).expect("entry").hops;

    assert!(
        !table.observe_passed_rebroadcast(&destination, observed_hops),
        "same-hop announces do not prove our rebroadcast was passed onward"
    );
    assert!(table.map.contains_key(&destination));
}

#[test]
fn zero_capacity_announce_cache_is_unbounded() {
    let mut table = AnnounceTable::new(0, 1);
    for byte in [1_u8, 2_u8] {
        let destination = AddressHash::new([byte; 16]);
        let packet = Packet { destination, ..Packet::default() };
        table.add_cached(&packet, destination, AddressHash::new([0x42; 16]));
    }

    assert_eq!(table.cache.len(), 2);
}
