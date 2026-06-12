#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_raw_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource");
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
    let mut config = TransportConfig::new("python-resource-interop", &rust_identity, true);
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
    let metadata = rmp_serde::to_vec(&String::from("rust-meta")).expect("metadata");
    let resource_hash = transport
        .send_resource(&link_id, b"rust-resource-data".to_vec(), Some(metadata))
        .await
        .expect("send resource");
    wait_for_outbound_resource_complete(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(8),
    )
    .await;
    wait_for_resource_ack(&seen, Duration::from_secs(8)).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_channel_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-channel-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "channel");
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    sleep(Duration::from_millis(50)).await;

    let channel = transport.channel(link_id);
    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    channel
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

    wait_for_python_message(&seen, Duration::from_secs(8)).await;
    let payload = rmp_serde::to_vec(&(String::from("python-1"), String::from("reply:hello-rust")))
        .expect("encode channel reply");
    channel.send(MSG_TYPE, payload).await.expect("send channel reply");

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python channel client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"reply:hello-rust\""),
        "python client did not report Rust channel reply: {stdout}"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_link_data_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-link-data-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-link-data-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "link-data");
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut received = transport.received_data_events();
    wait_for_link_data(&mut received, link_id, b"hello-rust", Duration::from_secs(8)).await;
    let destination_hash = { destination.lock().await.desc.address_hash };
    transport.send_to_in_links(&destination_hash, b"reply:hello-rust").await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python link-data client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reply:hello-rust"), "python client did not report link-data reply");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_request_response_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-request-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-request-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let mut transport = Transport::new(config);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "request");
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let mut received = transport.received_data_events();
    let (request_id, request_data) =
        wait_for_request(&mut received, link_id, Duration::from_secs(8)).await;
    assert_eq!(rmpv_to_string(&request_data).as_deref(), Some("hello-rust"));
    send_link_response(
        &transport,
        link_id,
        request_id,
        rmpv::Value::String("reply:hello-rust".into()),
    )
    .await
    .expect("send request response");

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python request client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reply:hello-rust"), "python client did not report request response");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_resource_backed_request_response_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-large-request-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-large-request-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let mut transport = Transport::new(config);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "large-request");
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
    let packed_request = wait_for_inbound_resource_data_or_child_exit(
        &mut resource_events,
        link_id,
        guard.child.as_mut().expect("python child"),
        Duration::from_secs(8),
    )
    .await;
    let request_data = parse_request_payload(&packed_request).expect("large request payload");
    let request_text = rmpv_to_string(&request_data).expect("large request text");
    assert!(request_text.starts_with("large:"));
    assert!(request_text.len() > 900);

    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(&address_hash(&packed_request));
    let response_payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::Binary(request_id.to_vec()),
        rmpv::Value::String(format!("reply:{request_text}").into()),
    ]))
    .expect("large response payload");
    let response_hash = transport
        .send_response_resource(&link_id, request_id.to_vec(), response_payload, None)
        .await
        .expect("send large response resource");
    wait_for_outbound_resource_complete(
        &mut resource_events,
        response_hash,
        Duration::from_secs(8),
    )
    .await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python large request client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"reply:large:"),
        "python client did not report large request response"
    );
}
