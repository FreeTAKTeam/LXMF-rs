#[test]
fn bootstrap_restores_python_path_table_for_path_lookup_rpc() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let mut identity_path = db_path.clone();
    identity_path.set_extension("identity");
    let local_identity =
        reticulum_daemon::identity_store::load_or_create_identity(&identity_path)
            .expect("seed daemon identity");
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    let (destination_hex, expected_iface_hex, expected_public_key_hex, expected_verifying_key_hex) =
        runtime.block_on(async {
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(&local_identity);
        let mut config =
            TransportConfig::new("bootstrap-path-restore-seed", &transport_identity, true);
        config.set_retransmit(true);
        let seed_transport = Transport::new(config);
        let iface_channel = seed_transport.iface_manager().lock().await.new_channel(16);
        let iface = *iface_channel.address();

        let remote_identity =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let mut remote_destination = rns_transport::destination::SingleInputDestination::new(
            remote_identity,
            DestinationName::new("lxmf", "delivery"),
        );
        let expected_identity = *remote_destination.identity.as_identity();
        let announce = remote_destination
            .announce(rand_core::OsRng, None)
            .expect("valid announce packet");
        let destination = announce.destination;
        let packet_hash_hex = hex::encode(announce.hash().as_slice());

        iface_channel
            .rx_channel
            .send(rns_transport::iface::RxMessage {
                address: iface,
                packet: announce,
                source: rns_transport::iface::IfaceSource::None,
            })
            .await
            .expect("seed announce");

        for _ in 0..20 {
            if seed_transport.has_path(&destination).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            seed_transport.has_path(&destination).await,
            "seed transport should learn remote path before saving"
        );
        assert_eq!(
            seed_transport
                .save_reticulum_path_table(temp.path())
                .await
                .expect("save path cache"),
            1
        );
        assert!(
            temp.path().join("destination_table").exists(),
            "seed should write Reticulum-compatible destination_table"
        );
        assert!(
            temp.path()
                .join("cache")
                .join("announces")
                .join(packet_hash_hex)
                .exists(),
            "seed should write Reticulum-compatible announce cache"
        );

        (
            hex::encode(destination.as_slice()),
            hex::encode(iface.as_slice()),
            hex::encode(expected_identity.public_key_bytes()),
            hex::encode(expected_identity.verifying_key_bytes()),
        )
    });

    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            None,
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });

    let status = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 501,
            method: "path_status".to_string(),
            params: Some(json!({ "destination": destination_hex })),
        })
        .expect("path_status rpc");
    assert!(status.error.is_none(), "path_status should succeed: {:?}", status.error);
    let result = status.result.expect("path_status result");
    assert_eq!(result["destination"].as_str(), Some(destination_hex.as_str()));
    assert_eq!(result["destination_hash"].as_str(), Some(destination_hex.as_str()));
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("found"));
    let expected_next_hop = format!("/{destination_hex}/");
    let expected_iface = format!("/{expected_iface_hex}/");
    assert_eq!(result["next_hop"].as_str(), Some(expected_next_hop.as_str()));
    assert_eq!(result["interface"].as_str(), Some(expected_iface.as_str()));
    assert_eq!(result["hops"].as_u64(), Some(0));

    let request = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 502,
            method: "request_path".to_string(),
            params: Some(json!({ "destination": destination_hex, "timeout_secs": 0 })),
        })
        .expect("request_path rpc");
    assert!(request.error.is_none(), "request_path should succeed: {:?}", request.error);
    let result = request.result.expect("request_path result");
    assert_eq!(result["status"].as_str(), Some("found"));
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["requested"].as_bool(), Some(false));

    let restore_status = path_table_restore_runtime_status(&context.daemon);
    assert_eq!(restore_status["status"].as_str(), Some("ok"));
    assert_eq!(restore_status["restored_active_paths"].as_u64(), Some(1));

    let restored_keys = context
        .daemon
        .announce_identity_keys(destination_hex.as_str())
        .expect("announce identity lookup")
        .expect("restored announce identity keys");
    assert_eq!(restored_keys.0, expected_public_key_hex);
    assert_eq!(restored_keys.1, expected_verifying_key_hex);
}

#[test]
fn bootstrap_skips_malformed_cached_announce_entry_without_restore_error() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let mut identity_path = db_path.clone();
    identity_path.set_extension("identity");
    let local_identity =
        reticulum_daemon::identity_store::load_or_create_identity(&identity_path)
            .expect("seed daemon identity");
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    let (destination_hex, bad_destination_hex) = runtime.block_on(async {
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(&local_identity);
        let mut config =
            TransportConfig::new("bootstrap-path-restore-bad-cache-seed", &transport_identity, true);
        config.set_retransmit(true);
        let seed_transport = Transport::new(config);
        let iface_channel = seed_transport.iface_manager().lock().await.new_channel(16);
        let iface = *iface_channel.address();

        let remote_identity =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let mut remote_destination = rns_transport::destination::SingleInputDestination::new(
            remote_identity,
            DestinationName::new("lxmf", "delivery"),
        );
        let announce = remote_destination
            .announce(rand_core::OsRng, None)
            .expect("valid announce packet");
        let destination = announce.destination;

        iface_channel
            .rx_channel
            .send(rns_transport::iface::RxMessage {
                address: iface,
                packet: announce,
                source: rns_transport::iface::IfaceSource::None,
            })
            .await
            .expect("seed announce");

        for _ in 0..20 {
            if seed_transport.has_path(&destination).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            seed_transport.has_path(&destination).await,
            "seed transport should learn remote path before saving"
        );
        assert_eq!(
            seed_transport
                .save_reticulum_path_table(temp.path())
                .await
                .expect("save path cache"),
            1
        );

        let bad_destination = AddressHash::new_from_hash(&rns_transport::hash::Hash::new_from_slice(
            b"bad-cache-destination",
        ));
        let bad_packet_hash =
            rns_transport::hash::Hash::new_from_slice(b"bad-cache-packet");
        append_corrupt_cache_path_row(temp.path(), bad_destination, bad_packet_hash);
        fs::write(
            cached_announce_path(temp.path(), &bad_packet_hash),
            b"not-msgpack-cached-announce",
        )
        .expect("write corrupt cached announce");

        (hex::encode(destination.as_slice()), hex::encode(bad_destination.as_slice()))
    });

    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            None,
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });

    let status = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 601,
            method: "path_status".to_string(),
            params: Some(json!({ "destination": destination_hex })),
        })
        .expect("path_status rpc");
    assert!(status.error.is_none(), "path_status should succeed: {:?}", status.error);
    let result = status.result.expect("path_status result");
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("found"));

    let skipped = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 602,
            method: "path_status".to_string(),
            params: Some(json!({ "destination": bad_destination_hex })),
        })
        .expect("skipped path_status rpc");
    assert!(skipped.error.is_none(), "path_status should succeed: {:?}", skipped.error);
    let result = skipped.result.expect("skipped path_status result");
    assert_eq!(result["known"].as_bool(), Some(false));
    assert_eq!(result["path_found"].as_bool(), Some(false));
    assert_eq!(result["status"].as_str(), Some("unknown"));

    let restore_status = path_table_restore_runtime_status(&context.daemon);
    assert_eq!(restore_status["status"].as_str(), Some("ok"));
    assert_eq!(restore_status["restored_active_paths"].as_u64(), Some(1));
}

#[test]
fn bootstrap_skips_missing_cached_announce_rows_without_restore_error() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let mut identity_path = db_path.clone();
    identity_path.set_extension("identity");
    let local_identity =
        reticulum_daemon::identity_store::load_or_create_identity(&identity_path)
            .expect("seed daemon identity");
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    let (destination_hex, missing_active_hex, missing_tunnel_hex) = runtime.block_on(async {
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(&local_identity);
        let mut config =
            TransportConfig::new("bootstrap-path-restore-missing-cache-seed", &transport_identity, true);
        config.set_retransmit(true);
        let seed_transport = Transport::new(config);
        let iface_channel = seed_transport.iface_manager().lock().await.new_channel(16);
        let iface = *iface_channel.address();

        let remote_identity =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let mut remote_destination = rns_transport::destination::SingleInputDestination::new(
            remote_identity,
            DestinationName::new("lxmf", "delivery"),
        );
        let announce = remote_destination
            .announce(rand_core::OsRng, None)
            .expect("valid announce packet");
        let destination = announce.destination;

        iface_channel
            .rx_channel
            .send(rns_transport::iface::RxMessage {
                address: iface,
                packet: announce,
                source: rns_transport::iface::IfaceSource::None,
            })
            .await
            .expect("seed announce");

        for _ in 0..20 {
            if seed_transport.has_path(&destination).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            seed_transport.has_path(&destination).await,
            "seed transport should learn remote path before saving"
        );
        assert_eq!(
            seed_transport
                .save_reticulum_path_table(temp.path())
                .await
                .expect("save path cache"),
            1
        );

        let missing_active = AddressHash::new_from_hash(&rns_transport::hash::Hash::new_from_slice(
            b"missing-active-cache",
        ));
        let missing_active_hash =
            rns_transport::hash::Hash::new_from_slice(b"missing-active-packet");
        append_corrupt_cache_path_row(temp.path(), missing_active, missing_active_hash);
        assert!(
            !cached_announce_path(temp.path(), &missing_active_hash).exists(),
            "test must exercise an active path row whose cached announce file is absent"
        );

        let missing_tunnel = AddressHash::new_from_hash(&rns_transport::hash::Hash::new_from_slice(
            b"missing-tunnel-cache",
        ));
        let missing_tunnel_hash =
            rns_transport::hash::Hash::new_from_slice(b"missing-tunnel-packet");
        append_missing_cache_tunnel_row(temp.path(), missing_tunnel, missing_tunnel_hash);
        assert!(
            !cached_announce_path(temp.path(), &missing_tunnel_hash).exists(),
            "test must exercise a tunnel path row whose cached announce file is absent"
        );

        (
            hex::encode(destination.as_slice()),
            hex::encode(missing_active.as_slice()),
            hex::encode(missing_tunnel.as_slice()),
        )
    });

    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            None,
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });

    assert_path_status(&context.daemon, &destination_hex, true, 701);
    assert_path_status(&context.daemon, &missing_active_hex, false, 702);
    assert_path_status(&context.daemon, &missing_tunnel_hex, false, 703);
    assert!(
        context
            .daemon
            .announce_identity_keys(missing_active_hex.as_str())
            .expect("missing active announce identity lookup")
            .is_none(),
        "missing active cached announce row must not restore identity material"
    );
    assert!(
        context
            .daemon
            .announce_identity_keys(missing_tunnel_hex.as_str())
            .expect("missing tunnel announce identity lookup")
            .is_none(),
        "missing tunnel cached announce row must not restore identity material"
    );

    let restore_status = path_table_restore_runtime_status(&context.daemon);
    assert_eq!(restore_status["status"].as_str(), Some("ok"));
    assert_eq!(restore_status["restored_active_paths"].as_u64(), Some(1));
}

#[test]
fn bootstrap_reports_path_table_restore_error_in_daemon_status() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    fs::write(temp.path().join("destination_table"), b"not-msgpack-path-table")
        .expect("write corrupt destination table");
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            None,
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });

    let restore_status = path_table_restore_runtime_status(&context.daemon);
    assert_eq!(restore_status["status"].as_str(), Some("error"));
    assert_eq!(restore_status["error"].as_str(), Some("decode path table"));
}

fn append_corrupt_cache_path_row(
    storage_path: &std::path::Path,
    destination: AddressHash,
    packet_hash: rns_transport::hash::Hash,
) {
    let table_path = storage_path.join("destination_table");
    let payload = fs::read(&table_path).expect("read destination table");
    let value: rmpv::Value =
        rmpv::decode::read_value(&mut std::io::Cursor::new(payload)).expect("decode msgpack");
    let rmpv::Value::Array(mut entries) = value else {
        panic!("destination_table must be an array");
    };
    let rmpv::Value::Array(mut bad_entry) =
        entries.first().expect("seed path row").clone()
    else {
        panic!("destination_table row must be an array");
    };
    bad_entry[0] = rmpv::Value::Binary(destination.as_slice().to_vec());
    bad_entry[2] = rmpv::Value::Binary(destination.as_slice().to_vec());
    bad_entry[7] = rmpv::Value::Binary(packet_hash.as_slice().to_vec());
    entries.push(rmpv::Value::Array(bad_entry));

    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &rmpv::Value::Array(entries)).expect("encode msgpack");
    fs::write(table_path, out).expect("write destination table");
}

fn append_missing_cache_tunnel_row(
    storage_path: &std::path::Path,
    destination: AddressHash,
    packet_hash: rns_transport::hash::Hash,
) {
    let tunnel_path = storage_path.join("tunnels");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_secs_f64();
    let path = rmpv::Value::Array(vec![
        rmpv::Value::Binary(destination.as_slice().to_vec()),
        rmpv::Value::F64(now),
        rmpv::Value::Binary(destination.as_slice().to_vec()),
        rmpv::Value::from(1_u64),
        rmpv::Value::F64(now + 60.0 * 60.0 * 8.0),
        rmpv::Value::Array(Vec::new()),
        rmpv::Value::Nil,
        rmpv::Value::Binary(packet_hash.as_slice().to_vec()),
    ]);
    let tunnel = rmpv::Value::Array(vec![
        rmpv::Value::Binary(
            rns_transport::hash::Hash::new_from_slice(b"missing-cache-tunnel")
                .as_slice()
                .to_vec(),
        ),
        rmpv::Value::Nil,
        rmpv::Value::Array(vec![path]),
        rmpv::Value::F64(now + 60.0 * 60.0 * 8.0),
    ]);
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &rmpv::Value::Array(vec![tunnel]))
        .expect("encode tunnel table");
    fs::write(tunnel_path, out).expect("write tunnel table");
}

fn cached_announce_path(
    storage_path: &std::path::Path,
    packet_hash: &rns_transport::hash::Hash,
) -> std::path::PathBuf {
    storage_path.join("cache").join("announces").join(hex::encode(packet_hash.as_slice()))
}

fn assert_path_status(daemon: &RpcDaemon, destination_hex: &str, expected_known: bool, id: u64) {
    let status = daemon
        .handle_rpc(RpcRequest {
            id,
            method: "path_status".to_string(),
            params: Some(json!({ "destination": destination_hex })),
        })
        .expect("path_status rpc");
    assert!(status.error.is_none(), "path_status should succeed: {:?}", status.error);
    let result = status.result.expect("path_status result");
    assert_eq!(result["known"].as_bool(), Some(expected_known));
    assert_eq!(result["path_found"].as_bool(), Some(expected_known));
    assert_eq!(
        result["status"].as_str(),
        Some(if expected_known { "found" } else { "unknown" })
    );
}

fn path_table_restore_runtime_status(daemon: &RpcDaemon) -> serde_json::Value {
    let status = daemon
        .handle_rpc(RpcRequest { id: 503, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon_status_ex rpc");
    assert!(status.error.is_none(), "daemon_status_ex should succeed: {:?}", status.error);
    let result = status.result.expect("daemon_status_ex result");
    let interfaces = result["interfaces"].as_array().expect("interfaces array");
    let daemon_transport = interfaces
        .iter()
        .find(|interface| interface["settings"]["_runtime"]["managed_by"] == "daemon_transport")
        .unwrap_or_else(|| panic!("daemon transport interface not found: {interfaces:?}"));
    daemon_transport["settings"]["_runtime"]["reticulum"]["path_table_restore"].clone()
}
