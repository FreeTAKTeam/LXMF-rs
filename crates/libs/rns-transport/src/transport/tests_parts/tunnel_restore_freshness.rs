fn tunnel_restore_random_blob(emitted: u64) -> [u8; crate::destination::RAND_HASH_LENGTH] {
    let mut blob = [0u8; crate::destination::RAND_HASH_LENGTH];
    blob[..5].copy_from_slice(b"tnnl!");
    blob[5..].copy_from_slice(&emitted.to_be_bytes()[3..]);
    blob
}

fn tunnel_id_for(identity: &PrivateIdentity, iface_hash: Hash) -> Hash {
    let public_identity = identity.as_identity();
    let mut material = Vec::new();
    material.extend_from_slice(public_identity.public_key_bytes());
    material.extend_from_slice(public_identity.verifying_key_bytes());
    material.extend_from_slice(iface_hash.as_slice());
    Hash::new_from_slice(&material)
}

#[tokio::test]
async fn tunnel_synthesize_preserves_fresher_active_path_over_older_restored_tunnel_path() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");
    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_id = tunnel_id_for(&tunnel_identity, iface_hash);
    let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
    let active_next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"active-next-hop"));
    let tunnel_next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"tunnel-next-hop"));

    let handler = transport.get_handler();
    let mut handler = handler.lock().await;
    assert_eq!(
        handler.tunnel_table.restore_python_entries(
            vec![super::tunnels::PythonTunnelEntry {
                tunnel_id,
                interface_hash: None,
                paths: vec![super::tunnels::PythonTunnelPathEntry {
                    destination,
                    timestamp_secs: 100.0,
                    received_from: tunnel_next_hop,
                    hops: 2,
                    expires_secs: 200.0,
                    random_blobs: vec![tunnel_restore_random_blob(100)],
                    interface_hash: None,
                    packet_hash: Hash::new_from_slice(b"tunnel-packet"),
                }],
                expires_secs: 200.0,
            }],
            std::time::Instant::now(),
            100.0,
        ),
        1
    );

    let active_packet = Packet {
        header: Header { packet_type: PacketType::Announce, hops: 2, ..Default::default() },
        destination,
        ..Default::default()
    };
    assert!(handler.path_table.handle_announce(
        &active_packet,
        Some(active_next_hop),
        iface,
        tunnel_restore_random_blob(101),
        |_| Some(crate::iface::InterfaceMode::Full),
    ));

    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;

    let entry = handler.path_table.get(&destination).expect("active path should remain");
    assert_eq!(entry.received_from, active_next_hop);
    assert_eq!(entry.iface, iface);
    assert_eq!(entry.hops, 2);
}

#[tokio::test]
async fn tunnel_synthesize_replaces_active_path_with_fresher_restored_tunnel_path() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");
    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_id = tunnel_id_for(&tunnel_identity, iface_hash);
    let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination-fresh"));
    let active_next_hop =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"active-next-hop-fresh"));
    let tunnel_next_hop =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"tunnel-next-hop-fresh"));
    let tunnel_packet_hash = Hash::new_from_slice(b"tunnel-packet-fresh");

    let handler = transport.get_handler();
    let mut handler = handler.lock().await;
    assert_eq!(
        handler.tunnel_table.restore_python_entries(
            vec![super::tunnels::PythonTunnelEntry {
                tunnel_id,
                interface_hash: None,
                paths: vec![super::tunnels::PythonTunnelPathEntry {
                    destination,
                    timestamp_secs: 100.0,
                    received_from: tunnel_next_hop,
                    hops: 2,
                    expires_secs: 200.0,
                    random_blobs: vec![tunnel_restore_random_blob(102)],
                    interface_hash: None,
                    packet_hash: tunnel_packet_hash,
                }],
                expires_secs: 200.0,
            }],
            std::time::Instant::now(),
            100.0,
        ),
        1
    );

    let active_packet = Packet {
        header: Header { packet_type: PacketType::Announce, hops: 2, ..Default::default() },
        destination,
        ..Default::default()
    };
    assert!(handler.path_table.handle_announce(
        &active_packet,
        Some(active_next_hop),
        iface,
        tunnel_restore_random_blob(101),
        |_| Some(crate::iface::InterfaceMode::Full),
    ));

    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;

    let entry = handler.path_table.get(&destination).expect("fresher tunnel path should restore");
    assert_eq!(entry.received_from, tunnel_next_hop);
    assert_eq!(entry.iface, iface);
    assert_eq!(entry.hops, 2);
    assert_eq!(entry.packet_hash, tunnel_packet_hash);
}
