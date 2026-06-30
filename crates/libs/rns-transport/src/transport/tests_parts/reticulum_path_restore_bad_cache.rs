#[tokio::test]
async fn reticulum_path_table_restore_skips_malformed_cached_announce_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let good = learn_cached_path(&transport, iface, "good").await;
    let bad_destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"bad-cache-dest"));
    let bad_packet_hash = Hash::new_from_slice(b"bad-cache-packet");

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    append_bad_path_table_entry(temp.path(), bad_destination, bad_packet_hash);
    corrupt_cached_announce_msgpack(temp.path(), &bad_packet_hash);

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");

    let restore_report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(restore_report.restored_active_paths, 1);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, good.destination);
    assert!(restored.has_path(&good.destination).await, "valid cached row should restore");
    assert!(restored.destination_identity(&good.destination).await.is_some());
    assert!(
        !restored.has_path(&bad_destination).await,
        "malformed cached announce row should be skipped without aborting restore"
    );
    assert!(restored.destination_identity(&bad_destination).await.is_none());
}

#[tokio::test]
async fn reticulum_path_table_restore_skips_missing_cached_announce_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let good = learn_cached_path(&transport, iface, "good-missing-cache").await;
    let missing_destination =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"missing-cache-dest"));
    let missing_packet_hash = Hash::new_from_slice(b"missing-cache-packet");

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    append_bad_path_table_entry(temp.path(), missing_destination, missing_packet_hash);
    assert!(
        !cached_announce_path(temp.path(), &missing_packet_hash).exists(),
        "test must exercise a path-table row whose cached announce file is absent"
    );

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");

    let restore_report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(restore_report.restored_active_paths, 1);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, good.destination);
    assert!(restored.has_path(&good.destination).await, "valid cached row should restore");
    assert!(restored.destination_identity(&good.destination).await.is_some());
    assert!(
        !restored.has_path(&missing_destination).await,
        "missing cached announce row should be skipped without aborting restore"
    );
    assert!(restored.destination_identity(&missing_destination).await.is_none());
}

#[tokio::test]
async fn reticulum_path_table_restore_skips_mismatched_cached_announce_destination() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let good = learn_cached_path(&transport, iface, "good-mismatch-cache").await;
    let mismatched_destination =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"mismatch-cache-dest"));

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    append_mismatched_path_table_entry(temp.path(), mismatched_destination);

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");

    let restore_report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(restore_report.restored_active_paths, 1);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, good.destination);
    assert!(restored.has_path(&good.destination).await, "valid cached row should restore");
    assert!(restored.destination_identity(&good.destination).await.is_some());
    assert!(
        !restored.has_path(&mismatched_destination).await,
        "cached announce for a different destination must not restore this path row"
    );
    assert!(restored.destination_identity(&mismatched_destination).await.is_none());
}

#[tokio::test]
async fn reticulum_tunnel_table_restore_skips_malformed_cached_announce_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;
    }

    let good = learn_cached_path(&transport, iface, "tunnel-good").await;
    let bad_destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"bad-tunnel-dest"));
    let bad_packet_hash = Hash::new_from_slice(b"bad-tunnel-packet");

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");
    append_bad_tunnel_path_entry(temp.path(), bad_destination, bad_packet_hash);
    corrupt_cached_announce_packet(temp.path(), &bad_packet_hash);

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    let restore_report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(restore_report.restored_active_paths, 0);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, good.destination);

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        )
        .await;
    }

    assert!(restored.has_path(&good.destination).await, "valid tunnel row should restore");
    assert!(restored.destination_identity(&good.destination).await.is_some());
    assert!(
        !restored.has_path(&bad_destination).await,
        "malformed tunnel cached announce row should be skipped"
    );
    assert!(restored.destination_identity(&bad_destination).await.is_none());
}

#[tokio::test]
async fn reticulum_tunnel_table_restore_skips_missing_cached_announce_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;
    }

    let good = learn_cached_path(&transport, iface, "tunnel-missing-good").await;
    let missing_destination =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"missing-tunnel-dest"));
    let missing_packet_hash = Hash::new_from_slice(b"missing-tunnel-packet");

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");
    append_bad_tunnel_path_entry(temp.path(), missing_destination, missing_packet_hash);
    assert!(
        !cached_announce_path(temp.path(), &missing_packet_hash).exists(),
        "test must exercise a tunnel row whose cached announce file is absent"
    );

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    let restore_report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(restore_report.restored_active_paths, 0);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, good.destination);

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        )
        .await;
    }

    assert!(restored.has_path(&good.destination).await, "valid tunnel row should restore");
    assert!(restored.destination_identity(&good.destination).await.is_some());
    assert!(
        !restored.has_path(&missing_destination).await,
        "missing tunnel cached announce row should be skipped"
    );
    assert!(restored.destination_identity(&missing_destination).await.is_none());
}

#[tokio::test]
async fn reticulum_tunnel_table_restore_skips_mismatched_cached_announce_destination() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;
    }

    let good = learn_cached_path(&transport, iface, "tunnel-mismatch-good").await;
    let mismatched_destination =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"mismatch-tunnel-dest"));

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");
    append_mismatched_tunnel_path_entry(temp.path(), mismatched_destination);

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    let restore_report = restored
        .restore_reticulum_path_table_report(temp.path())
        .await
        .expect("restore");
    assert_eq!(restore_report.restored_active_paths, 0);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, good.destination);

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        )
        .await;
    }

    assert!(restored.has_path(&good.destination).await, "valid tunnel row should restore");
    assert!(restored.destination_identity(&good.destination).await.is_some());
    assert!(
        !restored.has_path(&mismatched_destination).await,
        "cached tunnel announce for a different destination must not restore this path row"
    );
    assert!(restored.destination_identity(&mismatched_destination).await.is_none());
}

struct CachedPathSeed {
    destination: AddressHash,
}

async fn learn_cached_path(transport: &Transport, iface: AddressHash, aspect: &str) -> CachedPathSeed {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", aspect));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    CachedPathSeed { destination }
}

fn corrupt_cached_announce_msgpack(storage_path: &std::path::Path, packet_hash: &Hash) {
    std::fs::write(cached_announce_path(storage_path, packet_hash), b"not-msgpack")
        .expect("write corrupt cached announce msgpack");
}

fn corrupt_cached_announce_packet(storage_path: &std::path::Path, packet_hash: &Hash) {
    let value = rmpv::Value::Array(vec![rmpv::Value::Binary(b"not-a-packet".to_vec())]);
    let mut payload = Vec::new();
    rmpv::encode::write_value(&mut payload, &value).expect("encode corrupt cached packet");
    std::fs::write(cached_announce_path(storage_path, packet_hash), payload)
        .expect("write corrupt cached announce packet");
}

fn cached_announce_path(storage_path: &std::path::Path, packet_hash: &Hash) -> std::path::PathBuf {
    storage_path.join("cache").join("announces").join(hex::encode(packet_hash.as_slice()))
}

fn append_bad_path_table_entry(
    storage_path: &std::path::Path,
    destination: AddressHash,
    packet_hash: Hash,
) {
    let table_path = storage_path.join("destination_table");
    let payload = std::fs::read(&table_path).expect("read destination table");
    let mut entries =
        super::path_table::PathTable::decode_python_entries(&payload).expect("decode entries");
    let seed = entries.first().expect("valid seed entry");
    let bad_entry = super::path_table::PythonPathEntry {
        destination,
        timestamp_secs: seed.timestamp_secs,
        received_from: destination,
        hops: seed.hops,
        expires_secs: seed.expires_secs,
        random_blobs: seed.random_blobs.clone(),
        iface: seed.iface,
        interface_hash: seed.interface_hash,
        packet_hash,
    };
    entries.push(bad_entry);
    std::fs::write(
        table_path,
        super::path_table::PathTable::encode_python_entries(&entries).expect("encode entries"),
    )
    .expect("write destination table with bad cache row");
}

fn append_mismatched_path_table_entry(storage_path: &std::path::Path, destination: AddressHash) {
    let table_path = storage_path.join("destination_table");
    let payload = std::fs::read(&table_path).expect("read destination table");
    let mut entries =
        super::path_table::PathTable::decode_python_entries(&payload).expect("decode entries");
    let seed = entries.first().expect("valid seed entry");
    let mismatched_entry = super::path_table::PythonPathEntry {
        destination,
        timestamp_secs: seed.timestamp_secs,
        received_from: destination,
        hops: seed.hops,
        expires_secs: seed.expires_secs,
        random_blobs: seed.random_blobs.clone(),
        iface: seed.iface,
        interface_hash: seed.interface_hash,
        packet_hash: seed.packet_hash,
    };
    entries.push(mismatched_entry);
    std::fs::write(
        table_path,
        super::path_table::PathTable::encode_python_entries(&entries).expect("encode entries"),
    )
    .expect("write destination table with mismatched cache row");
}

fn append_bad_tunnel_path_entry(
    storage_path: &std::path::Path,
    destination: AddressHash,
    packet_hash: Hash,
) {
    let tunnel_path = storage_path.join("tunnels");
    let payload = std::fs::read(&tunnel_path).expect("read tunnels");
    let mut tunnels =
        super::tunnels::TunnelTable::decode_python_entries(&payload).expect("decode tunnels");
    let seed = &tunnels.first().expect("valid seed tunnel").paths[0];
    let bad_path = super::tunnels::PythonTunnelPathEntry {
        destination,
        timestamp_secs: seed.timestamp_secs,
        received_from: destination,
        hops: seed.hops,
        expires_secs: seed.expires_secs,
        random_blobs: seed.random_blobs.clone(),
        interface_hash: seed.interface_hash,
        packet_hash,
    };
    tunnels[0].paths.push(bad_path);
    std::fs::write(
        tunnel_path,
        super::tunnels::TunnelTable::encode_python_entries(&tunnels).expect("encode tunnels"),
    )
    .expect("write tunnel table with bad cache row");
}

fn append_mismatched_tunnel_path_entry(storage_path: &std::path::Path, destination: AddressHash) {
    let tunnel_path = storage_path.join("tunnels");
    let payload = std::fs::read(&tunnel_path).expect("read tunnels");
    let mut tunnels =
        super::tunnels::TunnelTable::decode_python_entries(&payload).expect("decode tunnels");
    let seed = &tunnels.first().expect("valid seed tunnel").paths[0];
    let mismatched_path = super::tunnels::PythonTunnelPathEntry {
        destination,
        timestamp_secs: seed.timestamp_secs,
        received_from: destination,
        hops: seed.hops,
        expires_secs: seed.expires_secs,
        random_blobs: seed.random_blobs.clone(),
        interface_hash: seed.interface_hash,
        packet_hash: seed.packet_hash,
    };
    tunnels[0].paths.push(mismatched_path);
    std::fs::write(
        tunnel_path,
        super::tunnels::TunnelTable::encode_python_entries(&tunnels).expect("encode tunnels"),
    )
    .expect("write tunnel table with mismatched cache row");
}
