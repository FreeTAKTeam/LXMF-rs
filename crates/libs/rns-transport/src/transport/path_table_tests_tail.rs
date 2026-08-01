use super::*;

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
