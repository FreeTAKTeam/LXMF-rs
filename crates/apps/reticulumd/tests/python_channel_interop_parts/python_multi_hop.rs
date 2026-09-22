/// Proves that two independent Python nodes can discover and exchange a
/// Channel message through two Rust TCP carrier interfaces owned by one
/// transport instance. The endpoint and client stay in separate Python
/// processes; Rust only supplies the production transport/router path.
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_python_channel_roundtrip_through_rust_transport() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let endpoint_port = free_tcp_port();
    let client_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let endpoint_config_dir = temp.path().join("python-rns-multi-hop-endpoint");
    let client_config_dir = temp.path().join("python-rns-multi-hop-client");
    fs::create_dir_all(&endpoint_config_dir).expect("endpoint config dir");
    fs::create_dir_all(&client_config_dir).expect("client config dir");
    write_python_client_config(&endpoint_config_dir, endpoint_port);
    write_python_client_config(&client_config_dir, client_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-multi-hop-channel", &rust_identity, true);
    config.set_transport_enabled(true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{endpoint_port}"), iface_manager.clone()),
        TcpServer::spawn,
    );
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{client_port}"), iface_manager),
        TcpServer::spawn,
    );
    wait_for_port(endpoint_port, Duration::from_secs(5)).await;
    wait_for_port(client_port, Duration::from_secs(5)).await;

    let mut endpoint = paths.spawn_endpoint(&endpoint_config_dir, "channel");
    let ready = read_ready(&mut endpoint).unwrap_or_else(|| {
        let status = endpoint.try_wait().expect("inspect Python endpoint status");
        let mut stderr = String::new();
        if let Some(mut pipe) = endpoint.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
        }
        panic!("Python multi-hop endpoint did not become ready: status={status:?} stderr={stderr}");
    });
    let _endpoint_guard = ChildGuard { child: Some(endpoint) };

    let destination_hash = ready.destination_hash;
    let client = paths.spawn_channel_client(&client_config_dir, &destination_hash, "channel");
    let mut client_guard = ChildGuard { child: Some(client) };
    let client = client_guard.child.take().expect("Python multi-hop client");
    let output = tokio::task::spawn_blocking(move || client.wait_with_output())
        .await
        .expect("join Python multi-hop client")
        .expect("wait for Python multi-hop client");

    if !output.status.success() {
        panic!(
            "Python multi-hop client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"reply:hello-rust\""),
        "Python multi-hop client did not receive the endpoint reply: {stdout}"
    );

}

/// A completed link must be replaceable by a fresh link while the same
/// two-carrier Rust forwarding path remains active. Both Channel exchanges
/// must reach the Python endpoint exactly once.
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_python_channel_reconnect_through_rust_transport() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let endpoint_port = free_tcp_port();
    let client_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let endpoint_config_dir = temp.path().join("python-rns-multi-hop-reconnect-endpoint");
    let client_config_dir = temp.path().join("python-rns-multi-hop-reconnect-client");
    fs::create_dir_all(&endpoint_config_dir).expect("endpoint config dir");
    fs::create_dir_all(&client_config_dir).expect("client config dir");
    write_python_client_config(&endpoint_config_dir, endpoint_port);
    write_python_client_config(&client_config_dir, client_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-multi-hop-channel-reconnect", &rust_identity, true);
    config.set_transport_enabled(true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{endpoint_port}"), iface_manager.clone()),
        TcpServer::spawn,
    );
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{client_port}"), iface_manager),
        TcpServer::spawn,
    );
    wait_for_port(endpoint_port, Duration::from_secs(5)).await;
    wait_for_port(client_port, Duration::from_secs(5)).await;

    let mut endpoint = paths.spawn_endpoint(&endpoint_config_dir, "channel");
    let ready = read_ready(&mut endpoint).expect("Python reconnect endpoint ready");
    let mut endpoint_guard = ChildGuard { child: Some(endpoint) };

    let client = paths.spawn_channel_client(
        &client_config_dir,
        &ready.destination_hash,
        "channel-reconnect",
    );
    let client = tokio::task::spawn_blocking(move || client.wait_with_output())
        .await
        .expect("join Python reconnect client")
        .expect("wait for Python reconnect client");
    assert!(
        client.status.success(),
        "Python reconnect client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&client.stdout),
        String::from_utf8_lossy(&client.stderr)
    );
    let stdout = String::from_utf8_lossy(&client.stdout);
    assert!(
        stdout.contains("\"reply:hello-reconnect\""),
        "Python reconnect client did not receive the second endpoint reply: {stdout}"
    );

    sleep(Duration::from_millis(250)).await;
    let mut endpoint = endpoint_guard.child.take().expect("Python reconnect endpoint");
    endpoint.kill().expect("stop Python reconnect endpoint");
    let endpoint = tokio::task::spawn_blocking(move || endpoint.wait_with_output())
        .await
        .expect("join Python reconnect endpoint")
        .expect("wait for Python reconnect endpoint");
    let endpoint_stderr = String::from_utf8_lossy(&endpoint.stderr);
    for (message, expected) in [
        ("python-1 hello-rust", 1),
        ("python-reconnect hello-reconnect", 1),
    ] {
        let received = endpoint_stderr
            .matches(&format!("python_channel_endpoint: received channel message {message}"))
            .count();
        assert_eq!(
            received, expected,
            "unexpected delivery count for {message}: {endpoint_stderr}"
        );
    }

    drop(transport);
}

/// The forwarding path must preserve Channel sequencing when a carrier
/// repeats an application frame. The proxy duplicates the first decoded
/// Channel packet from the Python client while leaving link setup and other
/// packet contexts unchanged; the endpoint must observe one logical message.
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_python_channel_duplicate_through_rust_transport() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let endpoint_port = free_tcp_port();
    let client_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let endpoint_config_dir = temp.path().join("python-rns-multi-hop-duplicate-endpoint");
    let client_config_dir = temp.path().join("python-rns-multi-hop-duplicate-client");
    fs::create_dir_all(&endpoint_config_dir).expect("endpoint config dir");
    fs::create_dir_all(&client_config_dir).expect("client config dir");
    write_python_client_config(&endpoint_config_dir, endpoint_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-multi-hop-channel-duplicate", &rust_identity, true);
    config.set_transport_enabled(true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{endpoint_port}"), iface_manager.clone()),
        TcpServer::spawn,
    );
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{client_port}"), iface_manager),
        TcpServer::spawn,
    );
    wait_for_port(endpoint_port, Duration::from_secs(5)).await;
    wait_for_port(client_port, Duration::from_secs(5)).await;

    let proxy =
        PythonResourceFaultProxy::bind(client_port, ResourceFaultMode::DuplicateChannelFirst)
            .await;
    write_python_client_config(&client_config_dir, proxy.port());

    let mut endpoint = paths.spawn_endpoint(&endpoint_config_dir, "channel");
    let ready = read_ready(&mut endpoint).expect("Python duplicate endpoint ready");
    let mut endpoint_guard = ChildGuard { child: Some(endpoint) };

    let client = paths.spawn_channel_client(&client_config_dir, &ready.destination_hash, "channel");
    let client = tokio::task::spawn_blocking(move || client.wait_with_output())
        .await
        .expect("join Python duplicate client")
        .expect("wait for Python duplicate client");
    assert!(
        client.status.success(),
        "Python duplicate client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&client.stdout),
        String::from_utf8_lossy(&client.stderr)
    );

    sleep(Duration::from_millis(250)).await;
    let mut endpoint = endpoint_guard.child.take().expect("Python duplicate endpoint");
    endpoint.kill().expect("stop Python duplicate endpoint");
    let endpoint = tokio::task::spawn_blocking(move || endpoint.wait_with_output())
        .await
        .expect("join Python duplicate endpoint")
        .expect("wait for Python duplicate endpoint");
    let endpoint_stderr = String::from_utf8_lossy(&endpoint.stderr);
    let received = endpoint_stderr
        .matches("python_channel_endpoint: received channel message python-1 hello-rust")
        .count();
    assert_eq!(received, 1, "duplicate Channel frame was delivered twice: {endpoint_stderr}");

    drop(proxy);
    drop(transport);
}

/// A duplicate link-request proof must be suppressed by the forwarding
/// transport before it reaches the originating Python node.
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_python_duplicate_link_request_proof_is_filtered_by_rust_transport() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let endpoint_port = free_tcp_port();
    let client_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let endpoint_config_dir = temp.path().join("python-rns-link-proof-duplicate-endpoint");
    let client_config_dir = temp.path().join("python-rns-link-proof-duplicate-client");
    fs::create_dir_all(&endpoint_config_dir).expect("endpoint config dir");
    fs::create_dir_all(&client_config_dir).expect("client config dir");

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-multi-hop-link-proof-duplicate", &rust_identity, true);
    config.set_transport_enabled(true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{endpoint_port}"), iface_manager.clone()),
        TcpServer::spawn,
    );
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{client_port}"), iface_manager),
        TcpServer::spawn,
    );
    wait_for_port(endpoint_port, Duration::from_secs(5)).await;
    wait_for_port(client_port, Duration::from_secs(5)).await;

    let proxy = PythonResourceFaultProxy::bind(
        endpoint_port,
        ResourceFaultMode::DuplicateLinkRequestProofFirst,
    )
    .await;
    let client_proxy =
        PythonResourceFaultProxy::bind(client_port, ResourceFaultMode::CountLinkRequestProof).await;
    write_python_client_config(&endpoint_config_dir, proxy.port());
    write_python_client_config(&client_config_dir, client_proxy.port());

    let mut endpoint = paths.spawn_endpoint(&endpoint_config_dir, "channel");
    let ready = read_ready(&mut endpoint).expect("Python duplicate-proof endpoint ready");
    let mut endpoint_guard = ChildGuard { child: Some(endpoint) };

    let client = paths.spawn_channel_client(&client_config_dir, &ready.destination_hash, "channel");
    let client = tokio::task::spawn_blocking(move || client.wait_with_output())
        .await
        .expect("join Python duplicate-proof client")
        .expect("wait for Python duplicate-proof client");
    assert!(
        client.status.success(),
        "Python duplicate-proof client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&client.stdout),
        String::from_utf8_lossy(&client.stderr)
    );
    assert_eq!(proxy.matched_frame_count(), 1, "proxy did not duplicate a link-request proof");
    assert_eq!(
        client_proxy.matched_frame_count(),
        1,
        "Rust transport forwarded a duplicate link-request proof to the Python client"
    );

    sleep(Duration::from_millis(250)).await;
    let mut endpoint = endpoint_guard.child.take().expect("Python duplicate-proof endpoint");
    endpoint.kill().expect("stop Python duplicate-proof endpoint");
    let endpoint = tokio::task::spawn_blocking(move || endpoint.wait_with_output())
        .await
        .expect("join Python duplicate-proof endpoint")
        .expect("wait for Python duplicate-proof endpoint");
    let endpoint_stderr = String::from_utf8_lossy(&endpoint.stderr);
    let deliveries = endpoint_stderr
        .matches("python_channel_endpoint: received channel message python-1 hello-rust")
        .count();
    assert_eq!(deliveries, 1, "link-proof duplication broke exactly-once channel delivery: {endpoint_stderr}");

    drop(proxy);
    drop(client_proxy);
    drop(transport);
}

/// The same two independent Python nodes also have to carry a split Resource
/// through the Rust transport, not only a Channel message. This exercises the
/// production forwarding path and the Python sender/receiver roles together.
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn python_to_python_resource_roundtrip_through_rust_transport() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let endpoint_port = free_tcp_port();
    let client_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let endpoint_config_dir = temp.path().join("python-rns-multi-hop-resource-endpoint");
    let client_config_dir = temp.path().join("python-rns-multi-hop-resource-client");
    fs::create_dir_all(&endpoint_config_dir).expect("endpoint config dir");
    fs::create_dir_all(&client_config_dir).expect("client config dir");
    write_python_client_config(&endpoint_config_dir, endpoint_port);
    write_python_client_config(&client_config_dir, client_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-multi-hop-resource", &rust_identity, true);
    config.set_transport_enabled(true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{endpoint_port}"), iface_manager.clone()),
        TcpServer::spawn,
    );
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{client_port}"), iface_manager),
        TcpServer::spawn,
    );
    wait_for_port(endpoint_port, Duration::from_secs(5)).await;
    wait_for_port(client_port, Duration::from_secs(5)).await;

    let mut endpoint = paths.spawn_endpoint(&endpoint_config_dir, "resource");
    let ready = read_ready(&mut endpoint).unwrap_or_else(|| {
        let status = endpoint.try_wait().expect("inspect Python endpoint status");
        let mut stderr = String::new();
        if let Some(mut pipe) = endpoint.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
        }
        panic!("Python multi-hop Resource endpoint did not become ready: status={status:?} stderr={stderr}");
    });
    let _endpoint_guard = ChildGuard { child: Some(endpoint) };

    let client = paths.spawn_multi_hop_resource_client(
        &client_config_dir,
        &ready.destination_hash,
        MAX_EFFICIENT_SIZE + 257,
        30.0,
    );
    let mut client_guard = ChildGuard { child: Some(client) };
    let client = client_guard.child.take().expect("Python multi-hop Resource client");
    let output = tokio::task::spawn_blocking(move || client.wait_with_output())
        .await
        .expect("join Python multi-hop Resource client")
        .expect("wait for Python multi-hop Resource client");

    if !output.status.success() {
        panic!(
            "Python multi-hop Resource client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"resource\": \"complete\""),
        "Python multi-hop client did not complete the forwarded Resource: {stdout}"
    );
    assert!(
        stdout.contains(&format!("\"size\": {}", MAX_EFFICIENT_SIZE + 257)),
        "Python multi-hop client reported the wrong Resource size: {stdout}"
    );
}
