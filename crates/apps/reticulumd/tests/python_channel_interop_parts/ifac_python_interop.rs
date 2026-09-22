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

async fn spawn_ifac_udp(
    transport: &Transport,
    bind_port: u16,
    forward_port: Option<u16>,
) -> (AddressHash, rns_transport::iface::udp::UdpRuntimeStatusHandle) {
    let bind_addr = format!("127.0.0.1:{bind_port}");
    let forward_addr = forward_port.map(|port| format!("127.0.0.1:{port}"));
    let interface = rns_transport::iface::udp::UdpInterface::new(bind_addr, forward_addr);
    let status = interface.runtime_status_handle();
    let iface_manager = transport.iface_manager();
    let mut manager = iface_manager.lock().await;
    let address = manager.spawn(interface, rns_transport::iface::udp::UdpInterface::spawn);
    assert!(manager
        .try_set_shared_config(address, ifac_shared_config())
        .expect("configure Rust UDP IFAC"));
    (address, status)
}

fn write_python_udp_config_with_ifac(
    dir: &std::path::Path,
    listen_port: u16,
    rust_port: u16,
) {
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[UDP IFAC Interface]]\n    type = UDPInterface\n    enabled = yes\n    listen_ip = 127.0.0.1\n    listen_port = {listen_port}\n    forward_ip = 127.0.0.1\n    forward_port = {rust_port}\n    networkname = {IFAC_NETWORK_NAME}\n    passphrase = {IFAC_PASSPHRASE}\n    ifac_size = {IFAC_SIZE_BITS}\n"
        ),
    )
    .expect("write Python UDP IFAC config");
}

#[tokio::test]
async fn udp_ifac_ingress_counts_and_rejects_malformed_frames_before_admission() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("ifac-udp-rejection", &identity, true));
    let port_reservation = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("reserve UDP port");
    let port = port_reservation.local_addr().expect("UDP local address").port();
    let sender = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("bind raw UDP sender");
    let destination = ("127.0.0.1", port);

    let ifac_context = ifac_shared_config()
        .ifac_context()
        .expect("derive IFAC context")
        .expect("IFAC enabled");
    let mut tampered = ifac_context.encode(&[0x01; 32]).expect("encode test IFAC frame");
    tampered[2] ^= 0x01;
    let mut wrong_credentials = ifac_shared_config();
    wrong_credentials.passphrase = Some("wrong-ifac-passphrase".to_string());
    let wrong_key = wrong_credentials
        .ifac_context()
        .expect("derive wrong-key IFAC context")
        .expect("wrong-key IFAC enabled")
        .encode(&[0x01; 32])
        .expect("encode wrong-key IFAC frame");
    let plaintext = [0x01; 32];
    let truncated = [0x80, 0x01];

    drop(port_reservation);
    let (address, runtime_status) = spawn_ifac_udp(&transport, port, None).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if runtime_status.to_json()["link_state"] == "bound" {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("UDP IFAC receiver did not bind");

    sender.send_to(&plaintext, destination).expect("send missing-flag frame");
    sender.send_to(&tampered, destination).expect("send tampered frame");
    sender.send_to(&wrong_key, destination).expect("send wrong-key frame");
    sender.send_to(&truncated, destination).expect("send truncated frame");

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = runtime_status.to_json();
            let snapshots = transport.interface_traffic_snapshots().await;
            let violations = snapshots
                .iter()
                .find(|snapshot| snapshot.address == address)
                .map(|snapshot| snapshot.ifac_violations)
                .unwrap_or_default();
            if status["decode_errors"] == 4 && violations == 4 {
                assert_eq!(status["packets_rx"], 0);
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("invalid UDP IFAC frames were not rejected and counted before packet admission");
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_rust_ifac_udp_channel_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();
    let rust_port_reservation = std::net::UdpSocket::bind(("127.0.0.1", 0))
        .expect("reserve Rust UDP port");
    let python_port_reservation = std::net::UdpSocket::bind(("127.0.0.1", 0))
        .expect("reserve Python UDP port");
    let rust_port = rust_port_reservation.local_addr().expect("Rust UDP address").port();
    let python_port = python_port_reservation.local_addr().expect("Python UDP address").port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-ifac-udp");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_udp_config_with_ifac(&py_config_dir, python_port, rust_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-rust-ifac-udp-interop", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    drop(rust_port_reservation);
    let (_, rust_status) = spawn_ifac_udp(&transport, rust_port, Some(python_port)).await;

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if rust_status.to_json()["link_state"] == "bound" {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Rust UDP IFAC carrier did not bind");

    drop(python_port_reservation);
    let mut child = paths.spawn_endpoint(&py_config_dir, "channel");
    let ready = read_ready(&mut child).expect("Python UDP endpoint ready");
    let _guard = ChildGuard { child: Some(child) };

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(12)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(10)).await;
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
        .expect("register UDP IFAC channel handler");

    let payload = rmp_serde::to_vec(&(String::from("rust-ifac-udp"), String::from("hello-python")))
        .expect("encode UDP IFAC channel message");
    let sequence = channel.send(MSG_TYPE, payload).await.expect("send UDP IFAC channel message");
    wait_for_reply_tuple(
        &seen,
        Duration::from_secs(10),
        "rust-ifac-udp",
        "reply:hello-python",
    )
    .await;
    wait_for_channel_delivery(&transport, link_id, sequence).await;
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
