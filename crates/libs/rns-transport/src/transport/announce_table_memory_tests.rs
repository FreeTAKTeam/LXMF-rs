#[test]
fn persistence_packet_handles_share_payload_without_changing_path_response_context() {
    let mut table = AnnounceTable::new(2, 1);
    let destination = AddressHash::new([0x31; 16]);
    let received_from = AddressHash::new([0x42; 16]);
    let packet = Packet {
        destination,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(b"cached payload"),
        ..Packet::default()
    };
    table.add_cached(&packet, destination, received_from);

    let first = table.shared_cached_packet_for_destination(&destination).expect("cached packet");
    let second = table.shared_cached_packet_for_destination(&destination).expect("cached packet");
    assert!(Arc::ptr_eq(&first, &second), "snapshot handles must share the owned packet");
    assert_eq!(Arc::strong_count(&first), 3, "table plus two snapshot handles");

    assert!(table.add_response(destination, received_from, 1));
    assert_eq!(first.context, PacketContext::None, "response mutation must not alter the cache");
    assert_eq!(
        table.pending_response_for_destination(&destination).expect("path response").packet.context,
        PacketContext::PathResponse
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 663 cached packet snapshot payload-copy estimate"]
fn diagnose_issue_663_cached_packet_snapshot_payload_copy() {
    const ROUTES: usize = 100_000;
    const PAYLOAD_BYTES: usize = 256;
    let received_from = AddressHash::new([0xa5; 16]);
    let payload = vec![0x5a; PAYLOAD_BYTES];
    let mut table = AnnounceTable::new(ROUTES, 1);
    for index in 0..ROUTES {
        let mut raw = [0; 16];
        raw[..8].copy_from_slice(&(index as u64).to_be_bytes());
        let destination = AddressHash::new(raw);
        let packet = Packet {
            destination,
            data: PacketDataBuffer::new_from_slice(&payload),
            ..Packet::default()
        };
        table.add_cached(&packet, destination, received_from);
    }

    let handles = table
        .cache
        .newer
        .as_ref()
        .expect("one cache generation")
        .keys()
        .map(|destination| {
            table.shared_cached_packet_for_destination(destination).expect("cached packet")
        })
        .collect::<Vec<_>>();
    assert_eq!(handles.len(), ROUTES);
    assert!(handles.iter().all(|packet| Arc::strong_count(packet) == 2));
    println!(
        "issue_663_cached_packet_snapshot routes={ROUTES} payload_bytes_per_route={PAYLOAD_BYTES} \
         packet_inline_bytes={} arc_handle_bytes={} snapshot_packet_inline_bytes_avoided={} \
         snapshot_arc_handle_bytes={} snapshot_payload_bytes_avoided={} shared_handles={}",
        std::mem::size_of::<Packet>(),
        std::mem::size_of::<Arc<Packet>>(),
        ROUTES * std::mem::size_of::<Packet>(),
        ROUTES * std::mem::size_of::<Arc<Packet>>(),
        ROUTES * PAYLOAD_BYTES,
        handles.len(),
    );
}
