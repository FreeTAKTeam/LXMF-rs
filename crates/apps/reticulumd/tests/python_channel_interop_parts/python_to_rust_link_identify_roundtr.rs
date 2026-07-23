#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_link_identify_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-identify-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-identify-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "identify");
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;
    let remote_identity =
        wait_for_link_identify(&mut in_events, link_id, Duration::from_secs(8)).await;
    assert_ne!(remote_identity.address_hash, *rust_identity.address_hash());

    let destination_hash = { destination.lock().await.desc.address_hash };
    transport.send_to_in_links(&destination_hash, b"reply:identified").await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python identify client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"identified\""), "python client did not report identify ack");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_channel_buffer_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-buffer-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-buffer-interop-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "buffer");
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

    let pair = Buffer::create_bidirectional_buffer(0, 0, transport.channel(link_id))
        .await
        .expect("buffer pair");
    wait_for_buffer_data(&pair.reader, b"hello-rust", Duration::from_secs(8)).await;
    let written = pair.writer.write_all(b"hello-rust back at you").await.expect("write reply");
    assert_eq!(written, "hello-rust back at you".len());

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python buffer client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello-rust back at you"),
        "python client did not report Rust buffer reply: {stdout}"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_raw_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-resource-interop-rust-server", &rust_identity, true);
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

    let child = paths.spawn_channel_client(&py_config_dir, &destination_hash, "resource");
    let mut guard = ChildGuard { child: Some(child) };

    let mut in_events = transport.in_link_events();
    let _link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(8),
    )
    .await;

    let mut resource_events = transport.resource_events();
    wait_for_inbound_resource_complete(
        &mut resource_events,
        b"hello-rust",
        "python-meta",
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
            "python resource client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"complete\""), "python client did not report resource completion");
}
