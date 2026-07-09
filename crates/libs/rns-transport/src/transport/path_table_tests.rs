use super::*;
use crate::packet::{Header, PacketDataBuffer, PacketType};

fn hash(seed: &[u8]) -> Hash {
    Hash::new_from_slice(seed)
}

fn addr(seed: &[u8]) -> AddressHash {
    AddressHash::new_from_hash(&hash(seed))
}

fn test_now() -> Instant {
    Instant::now() + DESTINATION_TIMEOUT + Duration::from_secs(1)
}

fn random_blob(prefix: &[u8; 5], emitted: u64) -> [u8; RAND_HASH_LENGTH] {
    let mut blob = [0u8; RAND_HASH_LENGTH];
    blob[..5].copy_from_slice(prefix);
    blob[5..].copy_from_slice(&emitted.to_be_bytes()[3..]);
    blob
}

fn add_path(table: &mut PathTable, destination: AddressHash, timestamp: Instant) {
    table.map.insert(
        destination,
        PathEntry {
            timestamp,
            received_from: destination,
            hops: 1,
            iface: addr(b"iface"),
            packet_hash: hash(b"packet"),
            random_blobs: Vec::new(),
            state: PathState::Unknown,
        },
    );
}

fn announce(destination: AddressHash, hops: u8, prefix: &[u8; 5], emitted: u64) -> Packet {
    let blob = random_blob(prefix, emitted);
    Packet {
        header: Header { packet_type: PacketType::Announce, hops, ..Default::default() },
        destination,
        data: PacketDataBuffer::new_from_slice(&blob),
        ..Default::default()
    }
}

#[test]
fn remove_stale_and_expire_path_match_expected_lifetimes() {
    let now = test_now();
    for (seed, mode, age, removed, remains) in [
        (b"ap".as_slice(), InterfaceMode::AccessPoint, AP_PATH_TIME, 1, false),
        (b"full".as_slice(), InterfaceMode::Full, AP_PATH_TIME, 0, true),
        (b"roam".as_slice(), InterfaceMode::Roaming, ROAMING_PATH_TIME, 1, false),
    ] {
        let destination = addr(seed);
        let mut table = PathTable::new();
        add_path(&mut table, destination, now - age - Duration::from_secs(1));
        assert_eq!(table.remove_stale(now, |_| Some(mode)), removed);
        assert_eq!(table.get(&destination).is_some(), remains);
    }

    let mut table = PathTable::new();
    let destination = addr(b"missing");
    add_path(&mut table, destination, now);
    assert_eq!(table.remove_stale(now, |_| None), 1);
    assert!(table.get(&destination).is_none());

    let other_destination = addr(b"other");
    add_path(&mut table, destination, now);
    add_path(&mut table, other_destination, now);
    assert!(table.expire_path(&destination));
    assert!(!table.expire_path(&destination));
    assert!(table.get(&destination).is_none());
    assert!(table.get(&other_destination).is_some());
}

#[test]
fn handle_announce_replacement_matches_python_freshness_rules() {
    let destination = addr(b"destination");
    let first_iface = addr(b"first-iface");
    let second_iface = addr(b"second-iface");
    let first_transport = addr(b"first-transport");
    let second_transport = addr(b"second-transport");

    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 2, b"first", 100),
        Some(first_transport),
        first_iface,
        random_blob(b"first", 100),
        |_| Some(InterfaceMode::Full),
    ));
    assert!(table.handle_announce(
        &announce(destination, 2, b"scnd!", 101),
        Some(second_transport),
        second_iface,
        random_blob(b"scnd!", 101),
        |_| Some(InterfaceMode::Full),
    ));
    let entry = table.get(&destination).expect("fresh equal-hop announce should replace");
    assert_eq!(entry.hops, 2);
    assert_eq!(entry.received_from, second_transport);
    assert_eq!(entry.iface, second_iface);
    assert_eq!(entry.random_blobs.len(), 2);

    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 3, b"dupe!", 100),
        None,
        first_iface,
        random_blob(b"dupe!", 100),
        |_| Some(InterfaceMode::Full),
    ));
    assert!(!table.handle_announce(
        &announce(destination, 1, b"dupe!", 100),
        None,
        second_iface,
        random_blob(b"dupe!", 100),
        |_| Some(InterfaceMode::Full),
    ));
    let entry = table.get(&destination).expect("initial announce should add a path");
    assert_eq!(entry.hops, 3);
    assert_eq!(entry.iface, first_iface);
    assert_eq!(entry.random_blobs.len(), 1);

    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 1, b"first", 100),
        None,
        first_iface,
        random_blob(b"first", 100),
        |_| Some(InterfaceMode::AccessPoint),
    ));
    table.map.get_mut(&destination).expect("path entry").timestamp -=
        AP_PATH_TIME + Duration::from_secs(1);
    assert!(table.handle_announce(
        &announce(destination, 4, b"later", 90),
        None,
        second_iface,
        random_blob(b"later", 90),
        |_| Some(InterfaceMode::AccessPoint),
    ));
    let entry = table.get(&destination).expect("expired higher-hop announce should replace");
    assert_eq!(entry.hops, 4);
    assert_eq!(entry.iface, second_iface);
    assert_eq!(entry.random_blobs.len(), 2);

    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 1, b"first", 100),
        None,
        first_iface,
        random_blob(b"first", 100),
        |_| Some(InterfaceMode::Full),
    ));
    assert!(table.handle_announce(
        &announce(destination, 4, b"later", 101),
        None,
        second_iface,
        random_blob(b"later", 101),
        |_| Some(InterfaceMode::Full),
    ));
    let entry = table.get(&destination).expect("newer emitted higher-hop announce should replace");
    assert_eq!(entry.hops, 4);
    assert_eq!(entry.iface, second_iface);
    assert_eq!(entry.random_blobs.len(), 2);

    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 1, b"first", 100),
        None,
        first_iface,
        random_blob(b"first", 100),
        |_| Some(InterfaceMode::Full),
    ));
    assert!(!table.handle_announce(
        &announce(destination, 4, b"first", 100),
        None,
        second_iface,
        random_blob(b"first", 100),
        |_| Some(InterfaceMode::Full),
    ));
    let entry = table.get(&destination).expect("initial route should remain responsive");
    assert_eq!(entry.hops, 1);
    assert_eq!(entry.iface, first_iface);

    assert!(table.mark_path_unresponsive(&destination));
    assert!(table.path_is_unresponsive(&destination));
    assert!(table.handle_announce(
        &announce(destination, 4, b"first", 100),
        Some(second_transport),
        second_iface,
        random_blob(b"first", 100),
        |_| Some(InterfaceMode::Full),
    ));
    let entry = table.get(&destination).expect("unresponsive route should be replaced");
    assert_eq!(entry.hops, 4);
    assert_eq!(entry.received_from, second_transport);
    assert_eq!(entry.iface, second_iface);
    assert!(!table.path_is_unresponsive(&destination));
}

#[test]
fn active_path_entries_keep_python_random_blob_window() {
    let destination = addr(b"destination-window");
    let iface = addr(b"iface-window");
    let transport = addr(b"transport-window");
    let mut table = PathTable::new();

    for emitted in 1..=70 {
        let mut prefix = [0u8; 5];
        prefix[4] = emitted;
        assert!(table.handle_announce(
            &announce(destination, 2, &prefix, u64::from(emitted)),
            Some(transport),
            iface,
            random_blob(&prefix, u64::from(emitted)),
            |_| Some(InterfaceMode::Full),
        ));
    }

    let entry = table.get(&destination).expect("path should exist");
    assert_eq!(entry.random_blobs.len(), MAX_RANDOM_BLOBS);
    assert_eq!(random_blob_timebase(entry.random_blobs.first().expect("oldest")), 7);
    assert_eq!(random_blob_timebase(entry.random_blobs.last().expect("newest")), 70);

    let exported = table.export_python_entries(
        Instant::now(),
        1_000_000.0,
        |_| Some((InterfaceMode::Full, hash(b"iface-full-hash"))),
    );
    assert_eq!(exported[0].random_blobs.len(), MAX_RANDOM_BLOBS);

    let encoded = PathTable::encode_python_entries(&exported).expect("encode");
    let decoded = PathTable::decode_python_entries(&encoded).expect("decode");
    assert_eq!(decoded[0].random_blobs.len(), MAX_RANDOM_BLOBS);
    assert_eq!(random_blob_timebase(decoded[0].random_blobs.first().expect("oldest")), 7);
    assert_eq!(random_blob_timebase(decoded[0].random_blobs.last().expect("newest")), 70);
}

#[test]
fn restore_python_entry_caps_random_blobs_to_python_memory_window() {
    let destination = addr(b"destination-restore-window");
    let blobs = (1..=70)
        .map(|emitted| {
            let mut prefix = [0u8; 5];
            prefix[4] = emitted;
            random_blob(&prefix, u64::from(emitted))
        })
        .collect();
    let mut table = PathTable::new();

    table.restore_python_entry(
        PythonPathEntry {
            destination,
            timestamp_secs: 1_000.0,
            received_from: destination,
            hops: 2,
            expires_secs: 2_000.0,
            random_blobs: blobs,
            iface: addr(b"iface-restore-window"),
            interface_hash: hash(b"iface-full-hash"),
            packet_hash: hash(b"packet-restore-window"),
        },
        Instant::now(),
        1_001.0,
    );

    let entry = table.get(&destination).expect("path should be restored");
    assert_eq!(entry.random_blobs.len(), MAX_RANDOM_BLOBS);
    assert_eq!(random_blob_timebase(entry.random_blobs.first().expect("oldest")), 7);
    assert_eq!(random_blob_timebase(entry.random_blobs.last().expect("newest")), 70);
}

#[test]
fn restore_tunnel_path_rejects_older_random_blob_over_fresh_active_path() {
    let destination = addr(b"destination-tunnel-fresh");
    let active_iface = addr(b"active-iface");
    let tunnel_iface = addr(b"tunnel-iface");
    let active_transport = addr(b"active-transport");
    let tunnel_transport = addr(b"tunnel-transport");
    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 2, b"fresh", 200),
        Some(active_transport),
        active_iface,
        random_blob(b"fresh", 200),
        |_| Some(InterfaceMode::Full),
    ));

    assert!(!table.restore_tunnel_path_with_random_blobs(TunnelPathRestore {
        destination,
        received_from: tunnel_transport,
        hops: 2,
        iface: tunnel_iface,
        packet_hash: hash(b"tunnel-old-packet"),
        random_blobs: vec![random_blob(b"older", 199)],
        existing_mode: Some(InterfaceMode::Full),
        now: Instant::now(),
    }));

    let entry = table.get(&destination).expect("fresh active path should remain");
    assert_eq!(entry.received_from, active_transport);
    assert_eq!(entry.iface, active_iface);
    assert_eq!(newest_random_blob_timebase(&entry.random_blobs), 200);
}

#[test]
fn restore_tunnel_path_replaces_with_newer_equal_hop_random_blob() {
    let destination = addr(b"destination-tunnel-newer");
    let active_iface = addr(b"active-iface-newer");
    let tunnel_iface = addr(b"tunnel-iface-newer");
    let active_transport = addr(b"active-transport-newer");
    let tunnel_transport = addr(b"tunnel-transport-newer");
    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 2, b"first", 200),
        Some(active_transport),
        active_iface,
        random_blob(b"first", 200),
        |_| Some(InterfaceMode::Full),
    ));

    assert!(table.restore_tunnel_path_with_random_blobs(TunnelPathRestore {
        destination,
        received_from: tunnel_transport,
        hops: 2,
        iface: tunnel_iface,
        packet_hash: hash(b"tunnel-new-packet"),
        random_blobs: vec![random_blob(b"newer", 201)],
        existing_mode: Some(InterfaceMode::Full),
        now: Instant::now(),
    }));

    let entry = table.get(&destination).expect("newer tunnel path should replace");
    assert_eq!(entry.received_from, tunnel_transport);
    assert_eq!(entry.iface, tunnel_iface);
    assert_eq!(newest_random_blob_timebase(&entry.random_blobs), 201);
}

#[test]
fn restore_tunnel_path_replaces_expired_active_path_even_with_more_hops() {
    let destination = addr(b"destination-tunnel-expired");
    let active_iface = addr(b"active-iface-expired");
    let tunnel_iface = addr(b"tunnel-iface-expired");
    let active_transport = addr(b"active-transport-expired");
    let tunnel_transport = addr(b"tunnel-transport-expired");
    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 1, b"first", 200),
        Some(active_transport),
        active_iface,
        random_blob(b"first", 200),
        |_| Some(InterfaceMode::AccessPoint),
    ));
    table.map.get_mut(&destination).expect("path entry").timestamp -=
        AP_PATH_TIME + Duration::from_secs(1);

    assert!(table.restore_tunnel_path_with_random_blobs(TunnelPathRestore {
        destination,
        received_from: tunnel_transport,
        hops: 4,
        iface: tunnel_iface,
        packet_hash: hash(b"tunnel-expired-packet"),
        random_blobs: vec![random_blob(b"equal", 200)],
        existing_mode: Some(InterfaceMode::AccessPoint),
        now: Instant::now(),
    }));

    let entry = table.get(&destination).expect("expired active path should replace");
    assert_eq!(entry.hops, 4);
    assert_eq!(entry.received_from, tunnel_transport);
    assert_eq!(entry.iface, tunnel_iface);
}

#[test]
fn restore_tunnel_path_defaults_missing_existing_mode_to_full_timeout() {
    let destination = addr(b"destination-tunnel-missing-mode");
    let active_iface = addr(b"active-iface-missing-mode");
    let tunnel_iface = addr(b"tunnel-iface-missing-mode");
    let active_transport = addr(b"active-transport-missing-mode");
    let tunnel_transport = addr(b"tunnel-transport-missing-mode");
    let mut table = PathTable::new();
    assert!(table.handle_announce(
        &announce(destination, 1, b"first", 200),
        Some(active_transport),
        active_iface,
        random_blob(b"first", 200),
        |_| Some(InterfaceMode::Full),
    ));
    let now = table.get(&destination).expect("path entry").timestamp
        + DESTINATION_TIMEOUT
        + Duration::from_secs(1);

    assert!(table.restore_tunnel_path_with_random_blobs(TunnelPathRestore {
        destination,
        received_from: tunnel_transport,
        hops: 4,
        iface: tunnel_iface,
        packet_hash: hash(b"tunnel-missing-mode-packet"),
        random_blobs: vec![random_blob(b"equal", 200)],
        existing_mode: None,
        now,
    }));

    let entry = table.get(&destination).expect("expired full-timeout path should replace");
    assert_eq!(entry.hops, 4);
    assert_eq!(entry.received_from, tunnel_transport);
    assert_eq!(entry.iface, tunnel_iface);
}

#[test]
fn restore_tunnel_path_caps_random_blobs_to_python_memory_window() {
    let destination = addr(b"destination-tunnel-window");
    let blobs = (1..=70)
        .map(|emitted| {
            let mut prefix = [0u8; 5];
            prefix[4] = emitted;
            random_blob(&prefix, u64::from(emitted))
        })
        .collect();
    let mut table = PathTable::new();

    assert!(table.restore_tunnel_path_with_random_blobs(TunnelPathRestore {
        destination,
        received_from: addr(b"tunnel-transport-window"),
        hops: 2,
        iface: addr(b"tunnel-iface-window"),
        packet_hash: hash(b"tunnel-window-packet"),
        random_blobs: blobs,
        existing_mode: None,
        now: Instant::now(),
    }));

    let entry = table.get(&destination).expect("tunnel path should restore");
    assert_eq!(entry.random_blobs.len(), MAX_RANDOM_BLOBS);
    assert_eq!(random_blob_timebase(entry.random_blobs.first().expect("oldest")), 7);
    assert_eq!(random_blob_timebase(entry.random_blobs.last().expect("newest")), 70);
}

#[test]
fn encode_python_entries_caps_oversized_random_blob_lists() {
    let destination = addr(b"destination-encode-window");
    let blobs = (1..=70)
        .map(|emitted| {
            let mut prefix = [0u8; 5];
            prefix[4] = emitted;
            random_blob(&prefix, u64::from(emitted))
        })
        .collect();

    let encoded = PathTable::encode_python_entries(&[PythonPathEntry {
        destination,
        timestamp_secs: 1_000.0,
        received_from: destination,
        hops: 2,
        expires_secs: 2_000.0,
        random_blobs: blobs,
        iface: addr(b"iface-encode-window"),
        interface_hash: hash(b"iface-full-hash"),
        packet_hash: hash(b"packet-encode-window"),
    }])
    .expect("encode");

    let decoded = PathTable::decode_python_entries(&encoded).expect("decode");
    assert_eq!(decoded[0].random_blobs.len(), MAX_RANDOM_BLOBS);
    assert_eq!(random_blob_timebase(decoded[0].random_blobs.first().expect("oldest")), 7);
    assert_eq!(random_blob_timebase(decoded[0].random_blobs.last().expect("newest")), 70);
}
