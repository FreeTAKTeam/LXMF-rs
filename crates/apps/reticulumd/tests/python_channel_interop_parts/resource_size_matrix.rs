use sha2::{Digest, Sha256};

use rns_transport::resource::MAX_EFFICIENT_SIZE;

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
