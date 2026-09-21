fn ifac_shared_config() -> InterfaceSharedConfig {
    InterfaceSharedConfig {
        ifac_size: Some(IFAC_SIZE_BITS),
        network_name: Some(IFAC_NETWORK_NAME.to_string()),
        passphrase: Some(IFAC_PASSPHRASE.to_string()),
        ..InterfaceSharedConfig::default()
    }
}

async fn spawn_ifac_tcp_client(transport: &Transport, server_port: u16) -> AddressHash {
    let iface_manager = transport.iface_manager();
    let mut manager = iface_manager.lock().await;
    let address = manager.spawn(
        TcpClient::new(format!("127.0.0.1:{server_port}")),
        TcpClient::spawn,
    );
    assert!(
        manager
            .try_set_shared_config(address, ifac_shared_config())
            .expect("configure Rust TCP client IFAC")
    );
    address
}

async fn spawn_ifac_tcp_server(transport: &Transport, server_port: u16) -> AddressHash {
    let iface_manager = transport.iface_manager();
    let mut manager = iface_manager.lock().await;
    let address = manager.spawn(
        TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager.clone()),
        TcpServer::spawn,
    );
    assert!(
        manager
            .try_set_shared_config(address, ifac_shared_config())
            .expect("configure Rust TCP server IFAC")
    );
    address
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_ifac_channel_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-ifac-channel");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config_with_ifac(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "channel");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-ifac-channel-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    spawn_ifac_tcp_client(&transport, server_port).await;

    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(8)).await;
    sleep(Duration::from_millis(100)).await;

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

    let payload =
        rmp_serde::to_vec(&(String::from("rust-ifac"), String::from("hello-python-ifac")))
            .expect("encode IFAC channel message");
    let sequence = channel.send(MSG_TYPE, payload).await.expect("send IFAC channel message");

    wait_for_reply_tuple(
        &seen,
        Duration::from_secs(8),
        "rust-ifac",
        "reply:hello-python-ifac",
    )
    .await;
    wait_for_channel_delivery(&transport, link_id, sequence).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_to_python_ifac_raw_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-ifac-resource");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config_with_ifac(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-ifac-resource-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    spawn_ifac_tcp_client(&transport, server_port).await;

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
        .expect("register IFAC resource acknowledgement handler");

    let mut resource_events = transport.resource_events();
    let metadata = rmp_serde::to_vec(&String::from("rust-meta")).expect("metadata");
    let resource_hash = transport
        .send_resource(&link_id, b"rust-resource-data".to_vec(), Some(metadata))
        .await
        .expect("send IFAC resource");
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
async fn python_to_rust_ifac_channel_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-ifac-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config_with_ifac(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-ifac-channel-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    spawn_ifac_tcp_server(&transport, server_port).await;
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
        .expect("register IFAC channel handler");

    wait_for_python_message(&seen, Duration::from_secs(8)).await;
    let payload = rmp_serde::to_vec(&(
        String::from("python-1"),
        String::from("reply:hello-rust"),
    ))
    .expect("encode IFAC channel reply");
    let sequence = channel.send(MSG_TYPE, payload).await.expect("send IFAC channel reply");
    wait_for_channel_delivery(&transport, link_id, sequence).await;

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python client")
        .expect("wait for python client");
    if !output.status.success() {
        panic!(
            "python IFAC channel client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("reply:hello-rust"),
        "python client did not report Rust IFAC channel reply: {stdout}"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_rust_ifac_raw_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-ifac-resource-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config_with_ifac(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-ifac-resource-rust-server", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    spawn_ifac_tcp_server(&transport, server_port).await;
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
            "python IFAC resource client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"complete\""),
        "python IFAC client did not report resource completion: {stdout}"
    );
}
