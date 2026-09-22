use rns_transport::destination::link::{LinkEvent, LinkStatus};

async fn run_rust_resource_fault(
    mode: ResourceFaultMode,
    expect_failure: bool,
    reader_backed: bool,
) {
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-reverse-resource-fault-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let proxy = PythonResourceFaultProxy::bind(server_port, mode).await;
    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-reverse-resource-fault-rust-sender", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{}", proxy.port())), TcpClient::spawn);

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
        .expect("register resource acknowledgement handler");

    let resource_size = 70_000;
    let payload = rust_resource_fixture(resource_size);
    let expected_digest = digest_hex(&payload);
    let mut resource_events = transport.resource_events();
    let resource_hash = if reader_backed {
        transport
            .send_resource_from_reader(
                &link_id,
                std::io::Cursor::new(payload),
                resource_size as u64,
                None,
            )
            .await
            .expect("send reader-backed resource through fault proxy")
    } else {
        transport
            .send_resource(&link_id, payload, None)
            .await
            .expect("send resource through fault proxy")
    };

    if expect_failure {
        wait_for_outbound_resource_failed_or_cancelled(
            &mut resource_events,
            resource_hash,
            Duration::from_secs(90),
        )
        .await;
    } else {
        wait_for_outbound_resource_complete(
            &mut resource_events,
            resource_hash,
            Duration::from_secs(30),
        )
        .await;
        wait_for_resource_digest_ack(
            &seen,
            resource_size,
            &expected_digest,
            Duration::from_secs(30),
        )
        .await;
    }

    drop(proxy);
    drop(transport);
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_resource_reverse_fault_matrix() {
    let _interop_guard = python_interop_guard().await;

    run_rust_resource_fault(ResourceFaultMode::DropFirst, false, false).await;
    run_rust_resource_fault(ResourceFaultMode::DuplicateFirst, false, false).await;
    run_rust_resource_fault(ResourceFaultMode::ReorderFirstTwo, false, false).await;
    run_rust_resource_fault(ResourceFaultMode::DropAll, true, false).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_reader_resource_fault_matrix() {
    let _interop_guard = python_interop_guard().await;

    run_rust_resource_fault(ResourceFaultMode::DropFirst, false, true).await;
    run_rust_resource_fault(ResourceFaultMode::DuplicateFirst, false, true).await;
    run_rust_resource_fault(ResourceFaultMode::ReorderFirstTwo, false, true).await;
    run_rust_resource_fault(ResourceFaultMode::DropAll, true, true).await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_link_timeout_and_reconnect_after_dropped_keepalives() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-link-timeout-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "channel");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let proxy = PythonResourceFaultProxy::bind(server_port, ResourceFaultMode::DropKeepAlive).await;
    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-link-timeout-rust", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{}", proxy.port())), TcpClient::spawn);

    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    let link_id = wait_for_out_link_active(&mut link_events, &link, Duration::from_secs(8)).await;

    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let event = link_events.recv().await.expect("link event");
            if event.id == link_id && matches!(event.event, LinkEvent::Closed) {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for Rust link timeout after dropped keepalives");
    assert_eq!(link.lock().await.status(), LinkStatus::Closed);

    let reconnect_link = transport.link(destination).await;
    let reconnect_id = wait_for_out_link_active(
        &mut link_events,
        &reconnect_link,
        Duration::from_secs(12),
    )
    .await;
    assert_ne!(reconnect_id, link_id, "timeout recovery reused the closed Link");

    drop(proxy);
    drop(transport);
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_link_establishment_timeout_after_dropped_request() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-link-establishment-timeout-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "channel");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let proxy = PythonResourceFaultProxy::bind(server_port, ResourceFaultMode::DropLinkRequest).await;
    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-link-establishment-timeout-rust", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    let transport = Transport::new(config);
    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpClient::new(format!("127.0.0.1:{}", proxy.port())), TcpClient::spawn);

    let destination = wait_for_announce(&transport, target_hash, Duration::from_secs(8)).await;
    assert!(
        transport.await_path(&target_hash, Duration::from_secs(8), None).await,
        "Python announce did not produce a usable path before link timeout"
    );
    let mut link_events = transport.out_link_events();
    let link = transport.link(destination).await;
    link.lock().await.set_establishment_timeout(Duration::from_secs(3));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = link_events.recv().await.expect("link event");
            if matches!(event.event, LinkEvent::Closed) {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for Rust link establishment timeout");
    assert_eq!(link.lock().await.status(), LinkStatus::Closed);

    drop(proxy);
    drop(transport);
}
