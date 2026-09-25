use sha2::{Digest, Sha256};

use rns_transport::resource::ResourceEventKind;
use rns_transport::resource::MAX_EFFICIENT_SIZE;

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_resource_compression_cap_boundary_probe() {
    let _interop_guard = python_interop_guard().await;
    let output = python_channel_interop_paths().resource_boundary_probe();
    assert!(
        output.status.success(),
        "pinned Python boundary probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let results: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Python boundary probe JSON");
    let at_limit = &results[0];
    assert_eq!(at_limit["size"], 64 * 1024 * 1024);
    assert_eq!(at_limit["total_size"], 64 * 1024 * 1024);
    assert_eq!(at_limit["segments"], 65);
    assert_eq!(at_limit["compressed"], true);
    assert_eq!(at_limit["status"], 0, "advertise=False leaves Python Resource at NONE");
    assert_eq!(at_limit["advertised"], false);
    assert_eq!(at_limit["parts_built"], 1, "probe constructs only the first split segment");

    let above_limit = &results[1];
    assert_eq!(above_limit["size"], 64 * 1024 * 1024 + 1);
    assert_eq!(above_limit["total_size"], 64 * 1024 * 1024 + 1);
    assert_eq!(above_limit["segments"], 65);
    assert_eq!(above_limit["compressed"], false);
    assert_eq!(above_limit["status"], 0, "probe suppresses network advertisement");
    assert_eq!(above_limit["advertised"], false);
    assert_eq!(above_limit["parts_built"], 1, "probe constructs only the first split segment");
}

fn rust_resource_fixture(size: usize) -> Vec<u8> {
    let mut state = 0x6051_5eed_u64;
    let mut data = vec![0u8; size];
    for byte in &mut data {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state >> 24) as u8;
    }
    data
}

fn digest_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_resource_size_matrix_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-size-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-size-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{server_port}")), TcpClient::spawn);

    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(8)).await;
    sleep(Duration::from_millis(100)).await;

    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    transport
        .channel(link_id)
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register channel handler");

    let mut resource_events = transport.resource_events();
    let sizes = [0, 1, MAX_EFFICIENT_SIZE, MAX_EFFICIENT_SIZE + 257, 50 * 1024 * 1024];
    for size in sizes {
        let payload = rust_resource_fixture(size);
        let expected_digest = digest_hex(&payload);
        let resource_hash = transport
            .send_resource(&link_id, payload, None)
            .await
            .expect("send resource");
        wait_for_outbound_resource_complete(
            &mut resource_events,
            resource_hash,
            Duration::from_secs(180),
        )
        .await;
        wait_for_resource_digest_ack(
            &seen,
            size,
            &expected_digest,
            Duration::from_secs(180),
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_resource_compression_defaults_match_pinned_python() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-compression");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource-compression");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;
    let target_hash = AddressHash::new_from_hex_string(&ready.destination_hash)
        .expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-compression-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{server_port}")), TcpClient::spawn);
    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(8)).await;
    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    transport
        .channel(link_id)
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register channel handler");

    let mut resource_events = transport.resource_events();
    let split_metadata = rmp_serde::to_vec(&"python-meta").expect("encode split Resource metadata");
    let mut cases = vec![
        ("compressible-default", b"resource compression default ".repeat(4096), true, true, None),
        ("incompressible-default", rust_resource_fixture(64 * 1024), true, false, None),
        (
            "compressible-split-segments",
            vec![b'R'; MAX_EFFICIENT_SIZE * 2],
            true,
            true,
            Some(split_metadata),
        ),
        (
            "incompressible-split-segments",
            rust_resource_fixture(MAX_EFFICIENT_SIZE * 2),
            true,
            false,
            None,
        ),
        ("compressible-disabled", b"resource compression disabled ".repeat(4096), false, false, None),
    ];
    for (label, payload, auto_compress, expected_compressed, metadata) in cases.drain(..) {
        let digest = digest_hex(&payload);
        let metadata_wire_size = metadata.as_ref().map(|value| value.len() + 3).unwrap_or(0);
        let total_size = payload.len() + metadata_wire_size;
        // Metadata reduces the first segment's source-data slice. Here that
        // creates a tiny third tail segment; the Python endpoint reports the
        // final segment's compression flag, which is false when compression
        // would grow its 15-byte payload. The Rust unit regression separately
        // checks that the first, metadata-bearing segment is compressed.
        let reported_compressed = if metadata_wire_size > 0 { false } else { expected_compressed };
        let resource_hash = transport
            .send_resource_with_compression(&link_id, payload.clone(), metadata, auto_compress)
            .await
        .expect("send Resource");
        wait_for_outbound_resource_complete(&mut resource_events, resource_hash, Duration::from_secs(30)).await;
        let expected = if metadata_wire_size > 0 {
            format!(
                "resource-sha256-metadata:{}:{digest}:{total_size}:python-meta:total_size={total_size}:compressed={reported_compressed}",
                payload.len(),
            )
        } else {
            format!(
                "resource-sha256:{}:{digest}:total_size={total_size}:compressed={reported_compressed}",
                payload.len(),
            )
        };
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if seen.lock().expect("seen lock").iter().any(|(id, data)| {
                    id == "rust-resource" && data == &expected
                }) {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "Python did not confirm {label} compression: expected={expected}; seen={:?}",
                seen.lock().expect("seen lock")
            )
        });
    }

    // The metadata wire prefix is part of the compressed Resource payload,
    // while the advertisement's logical size remains data + metadata. Verify
    // both properties against Python's production Resource receiver.
    let payload = b"compressed resource with metadata ".repeat(2048);
    let metadata = rmp_serde::to_vec(&"python-meta").expect("encode Resource metadata");
    let metadata_wire_size = metadata.len() + 3;
    let digest = digest_hex(&payload);
    let resource_hash = transport
        .send_resource(&link_id, payload.clone(), Some(metadata))
        .await
        .expect("send metadata-bearing Resource");
    wait_for_outbound_resource_complete(&mut resource_events, resource_hash, Duration::from_secs(30)).await;
    let expected = format!(
        "resource-sha256-metadata:{}:{digest}:{}:python-meta:total_size={}:compressed=true",
        payload.len(),
        payload.len() + metadata_wire_size,
        payload.len() + metadata_wire_size,
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if seen
                .lock()
                .expect("seen lock")
                .iter()
                .any(|(id, data)| id == "rust-resource" && data == &expected)
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Python did not confirm compressed metadata accounting: {expected}"));
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_resource_compression_defaults_match_rust() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-compression-rust", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager), TcpServer::spawn);
    wait_for_port(server_port, Duration::from_secs(5)).await;
    let destination = transport
        .add_destination(rust_identity.clone(), DestinationName::new("test", "channel"))
        .await;
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };
    let mut resource_events = transport.resource_events();

    for (kind, size, should_compress) in [
        ("resource-compression-compressible", 64 * 1024, true),
        ("resource-compression-incompressible", 64 * 1024, false),
        ("resource-compression-compressible", MAX_EFFICIENT_SIZE * 2, true),
        ("resource-compression-incompressible", MAX_EFFICIENT_SIZE * 2, false),
        ("resource-compression-disabled", 64 * 1024, false),
    ] {
        let py_config_dir = temp.path().join(kind);
        fs::create_dir_all(&py_config_dir).expect("python config dir");
        write_python_client_config(&py_config_dir, server_port);
        let child = paths.spawn_resource_client_with_kind(
            &py_config_dir,
            &destination_hash,
            kind,
            size,
            45.0,
        );
        let mut guard = ChildGuard { child: Some(child) };
        let mut in_events = transport.in_link_events();
        let link_id = wait_for_in_link_active_with_announces(
            &transport,
            &destination,
            &mut in_events,
            Duration::from_secs(8),
        )
        .await;
        let complete = wait_for_inbound_resource_data(
            &mut resource_events,
            link_id,
            Duration::from_secs(30),
        )
        .await;
        assert_eq!(complete.data.len(), size, "payload length for {kind}");
        let received_digest = digest_hex(&complete.data);
        let child = guard.child.take().expect("Python resource client");
        let output = tokio::task::spawn_blocking(move || child.wait_with_output())
            .await
            .expect("join Python client")
            .expect("wait for Python client");
        assert!(output.status.success(), "Python sender failed: {}", String::from_utf8_lossy(&output.stderr));
        let output = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.contains(&format!("\"sha256\": \"{received_digest}\"")),
            "Python/Rust payload digest differed for {kind}: {output}"
        );
        assert!(
            output.contains(&format!("\"compressed\": {should_compress}")),
            "Python compression decision for {kind} differed: {output}"
        );
    }
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_resource_compression_threshold_matches_pinned_python() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-compression-limit");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);
    let mut child = paths.spawn_endpoint(&py_config_dir, "resource-compression");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash = AddressHash::new_from_hex_string(&ready.destination_hash)
        .expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-compression-limit", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{server_port}")), TcpClient::spawn);
    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(8)).await;
    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    transport
        .channel(link_id)
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register channel handler");

    let mut resource_events = transport.resource_events();
    for (label, size, compressed) in [
        ("at-compression-threshold", 64 * 1024 * 1024, true),
        ("above-compression-threshold", 64 * 1024 * 1024 + 1, false),
    ] {
        let payload = vec![b'R'; size];
        let digest = digest_hex(&payload);
        let resource_hash = transport
            .send_resource(&link_id, payload, None)
            .await
            .expect("send Resource at and above the compression threshold");
        wait_for_outbound_resource_complete(
            &mut resource_events,
            resource_hash,
            Duration::from_secs(240),
        )
        .await;
        let expected = format!(
            "resource-sha256:{size}:{digest}:total_size={size}:compressed={compressed}"
        );
        tokio::time::timeout(Duration::from_secs(240), async {
            loop {
                if seen.lock().expect("seen lock").iter().any(|(id, data)| {
                    id == "rust-resource" && data == &expected
                }) {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Python did not confirm {label} compression: {expected}"));
    }
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_resource_compression_limit_matches_rust() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-compression-limit-rust", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager), TcpServer::spawn);
    wait_for_port(server_port, Duration::from_secs(5)).await;
    let destination = transport
        .add_destination(rust_identity.clone(), DestinationName::new("test", "channel"))
        .await;
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };
    let py_config_dir = temp.path().join("python-resource-compression-at-limit");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);
    let child = paths.spawn_resource_client_with_kind(
        &py_config_dir,
        &destination_hash,
        "resource-compression-threshold",
        64 * 1024 * 1024,
        240.0,
    );
    let mut guard = ChildGuard { child: Some(child) };
    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let complete = wait_for_inbound_resource_data(
        &mut transport.resource_events(),
        link_id,
        Duration::from_secs(240),
    )
    .await;
    assert_eq!(complete.data.len(), 64 * 1024 * 1024, "received logical size");
    let received_digest = digest_hex(&complete.data);
    let child = guard.child.take().expect("Python resource client");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join Python client")
        .expect("wait for Python client");
    assert!(output.status.success(), "Python sender failed: {}", String::from_utf8_lossy(&output.stderr));
    let output = String::from_utf8_lossy(&output.stdout);
    assert!(output.contains("\"size\": 67108864"), "Python size differed: {output}");
    assert!(output.contains("\"total_size\": 67108864"), "Python accounting differed: {output}");
    assert!(output.contains(&format!("\"sha256\": \"{received_digest}\"")), "Python/Rust digest differed: {output}");
    assert!(output.contains("\"compressed\": true"), "Python compression decision differed: {output}");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_resource_size_matrix_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-size-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager), TcpServer::spawn);
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let destination = transport
        .add_destination(rust_identity.clone(), DestinationName::new("test", "channel"))
        .await;
    let mut resource_events = transport.resource_events();
    let sizes = [0, 1, MAX_EFFICIENT_SIZE, MAX_EFFICIENT_SIZE + 257, 50 * 1024 * 1024];
    for size in sizes {
        let py_config_dir = temp.path().join(format!("python-rns-resource-size-client-{size}"));
        fs::create_dir_all(&py_config_dir).expect("python config dir");
        write_python_client_config(&py_config_dir, server_port);
        let destination_hash = {
            let destination = destination.lock().await;
            hex::encode(destination.desc.address_hash.as_slice())
        };
        let child = paths.spawn_resource_client(&py_config_dir, &destination_hash, size, 180.0);
        let mut guard = ChildGuard { child: Some(child) };
        let mut in_events = transport.in_link_events();
        let link_id = wait_for_in_link_active_with_announces(
            &transport,
            &destination,
            &mut in_events,
            Duration::from_secs(20),
        )
        .await;
        let complete =
            wait_for_inbound_resource_data(&mut resource_events, link_id, Duration::from_secs(180))
                .await;

        let child = guard.child.take().expect("python child");
        let output = tokio::task::spawn_blocking(move || child.wait_with_output())
            .await
            .expect("join python client")
            .expect("wait for python client");
        assert!(
            output.status.success(),
            "python resource client failed for {size} bytes\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report = String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .expect("python resource completion report");
        assert_eq!(report["resource"], "complete");
        assert_eq!(report["size"].as_u64(), Some(size as u64));
        assert_eq!(
            report["sha256"].as_str(),
            Some(digest_hex(&complete.data).as_str()),
            "Rust received a different payload for the {size}-byte Python transfer"
        );
        assert_eq!(complete.data.len(), size);
    }
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_resource_metadata_boundary_matches_rust_accounting() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-metadata-boundary", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager), TcpServer::spawn);
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let destination = transport
        .add_destination(rust_identity.clone(), DestinationName::new("test", "channel"))
        .await;
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };
    let metadata = rmp_serde::to_vec(&String::from("python-meta")).expect("metadata encoding");
    let metadata_wire_size = metadata.len() + 3;
    let payload_size = MAX_EFFICIENT_SIZE - metadata_wire_size + 1;
    let py_config_dir = temp.path().join("python-rns-resource-metadata-boundary");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);
    let child = paths.spawn_resource_client(&py_config_dir, &destination_hash, payload_size, 45.0);
    let mut guard = ChildGuard { child: Some(child) };
    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut resource_events = transport.resource_events();
    let mut segment_indexes = Vec::new();
    let mut total_data_size = None;
    let complete = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let event = resource_events.recv().await.expect("resource event");
            if event.link_id != link_id {
                continue;
            }
            match event.kind {
                ResourceEventKind::SegmentComplete(progress) => {
                    segment_indexes.push(progress.segment_index);
                    assert_eq!(progress.total_segments, 2);
                    total_data_size = Some(progress.total_data_size);
                }
                ResourceEventKind::Complete(complete) => break complete,
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for metadata-boundary Resource");

    assert_eq!(complete.data.len(), payload_size);
    assert_eq!(total_data_size, Some((MAX_EFFICIENT_SIZE + 1) as u64));
    assert_eq!(segment_indexes, [1, 2]);
    assert_eq!(complete.metadata.as_deref(), Some(metadata.as_slice()));
    let digest = digest_hex(&complete.data);

    let child = guard.child.take().expect("Python resource client");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join Python client")
        .expect("wait for Python client");
    assert!(output.status.success(), "Python sender failed: {}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .expect("Python Resource report");
    assert_eq!(report["size"].as_u64(), Some(payload_size as u64));
    assert_eq!(report["sha256"].as_str(), Some(digest.as_str()));
    assert_eq!(report["total_size"].as_u64(), Some((MAX_EFFICIENT_SIZE + 1) as u64));
    assert_eq!(report["segments"].as_u64(), Some(2));
    assert_eq!(report["metadata"], "python-meta");
}
