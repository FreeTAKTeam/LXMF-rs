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
