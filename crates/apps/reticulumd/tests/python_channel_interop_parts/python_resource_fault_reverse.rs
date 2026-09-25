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

    let proxy_mode = if expect_failure {
        ResourceFaultMode::DropAllResourceTraffic
    } else {
        mode
    };
    let proxy = PythonResourceFaultProxy::bind(server_port, proxy_mode).await;
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
        wait_for_outbound_resource_failed_rejected_or_cancelled(
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

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let proxy =
        PythonResourceFaultProxy::bind(server_port, ResourceFaultMode::DropResourceAndKeepAlive).await;
    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-link-timeout-rust", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(30);
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

    let mut resource_events = transport.resource_events();
    let stalled_payload = rust_resource_fixture(70_000);
    let stalled_hash = transport
        .send_resource(&link_id, stalled_payload, None)
        .await
        .expect("start Resource before dropping Link keepalives");

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
    wait_for_outbound_resource_failed(&mut resource_events, stalled_hash, Duration::from_secs(10)).await;

    proxy.resume_traffic();

    let reconnect_link = transport.link(destination).await;
    let reconnect_id = wait_for_out_link_active(
        &mut link_events,
        &reconnect_link,
        Duration::from_secs(12),
    )
    .await;
    assert_ne!(reconnect_id, link_id, "timeout recovery reused the closed Link");

    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    transport
        .channel(reconnect_id)
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register resource acknowledgement handler on recovered link");

    let payload = rust_resource_fixture(70_000);
    let expected_digest = digest_hex(&payload);
    let expected_size = payload.len();
    let resource_hash = transport
        .send_resource(&reconnect_id, payload, None)
        .await
        .expect("send Resource over recovered link");
    wait_for_outbound_resource_complete(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(30),
    )
    .await;
    wait_for_resource_digest_ack(
        &seen,
        expected_size,
        &expected_digest,
        Duration::from_secs(30),
    )
    .await;

    drop(proxy);
    drop(transport);
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_initiated_link_timeout_fails_then_recovers_inbound_resource() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-inbound-resource-link-timeout");
    fs::create_dir_all(&py_config_dir).expect("python config dir");

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-inbound-resource-link-timeout", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(30);
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(format!("127.0.0.1:{server_port}"), iface_manager),
        TcpServer::spawn,
    );
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let destination = transport
        .add_destination(rust_identity, DestinationName::new("test", "channel"))
        .await;
    let destination_hash = {
        let destination = destination.lock().await;
        hex::encode(destination.desc.address_hash.as_slice())
    };
    let proxy = PythonResourceFaultProxy::bind(
        server_port,
        ResourceFaultMode::DropResourcePartsAndKeepAlive,
    )
    .await;
    write_python_client_config(&py_config_dir, proxy.port());

    let child = paths.spawn_resource_client(&py_config_dir, &destination_hash, 70_000, 90.0);
    let _guard = ChildGuard { child: Some(child) };
    let mut in_events = transport.in_link_events();
    let link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(12),
    )
    .await;
    let mut resource_events = transport.resource_events();
    let failure = wait_for_inbound_resource_failure(
        &mut resource_events,
        link_id,
        Duration::from_secs(60),
    )
    .await;
    assert!(!failure.trim().is_empty(), "inbound failure must report a reason");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = in_events.recv().await.expect("inbound link event");
            if event.id == link_id && matches!(event.event, LinkEvent::Closed) {
                return;
            }
        }
    })
    .await
    .expect("inbound link did not close after keepalive loss");

    let recovery_proxy = PythonResourceFaultProxy::bind(
        server_port,
        ResourceFaultMode::DropResourcePartsAndKeepAlive,
    )
    .await;
    recovery_proxy.resume_traffic();
    let recovery_config_dir = temp.path().join("python-inbound-resource-recovery");
    fs::create_dir_all(&recovery_config_dir).expect("Python recovery config dir");
    write_python_client_config(&recovery_config_dir, recovery_proxy.port());
    let child = paths.spawn_resource_client(&recovery_config_dir, &destination_hash, 70_000, 90.0);
    let mut recovery_guard = ChildGuard { child: Some(child) };
    let recovery_link_id = wait_for_in_link_active_with_announces(
        &transport,
        &destination,
        &mut in_events,
        Duration::from_secs(12),
    )
    .await;
    assert_ne!(recovery_link_id, link_id, "recovery must use a fresh Link");
    let complete = wait_for_inbound_resource_data(
        &mut resource_events,
        recovery_link_id,
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(complete.data.len(), 70_000);
    let received_digest = digest_hex(&complete.data);

    let child = recovery_guard.child.take().expect("Python recovery client");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join Python recovery client")
        .expect("wait for Python recovery client");
    assert!(
        output.status.success(),
        "Python recovery client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&received_digest),
        "Python sender checksum must match recovered Rust Resource bytes"
    );

    drop(proxy);
    drop(recovery_proxy);
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

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource-shutdown");
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
    config.set_resource_retry_interval_secs(1);
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

    // The first Link never completed because its request was dropped. Reusing
    // the same carrier after the fault is removed must replace that terminal
    // Link and permit an actual Resource exchange with the pinned peer.
    proxy.resume_traffic();
    let recovered_link = transport.link(destination).await;
    let recovered_id = wait_for_out_link_active(
        &mut link_events,
        &recovered_link,
        Duration::from_secs(12),
    )
    .await;
    assert_ne!(recovered_id, *link.lock().await.id(), "timeout recovery must create a fresh Link");

    let seen = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
    let seen_clone = seen.clone();
    transport
        .channel(recovered_id)
        .register_handler(MSG_TYPE, move |envelope| {
            if let Ok(decoded) = rmp_serde::from_slice::<(String, String)>(&envelope.payload) {
                seen_clone.lock().expect("seen lock").push(decoded);
                true
            } else {
                false
            }
        })
        .await
        .expect("register acknowledgement handler on recovered Link");
    let payload = rust_resource_fixture(256);
    let expected_digest = digest_hex(&payload);
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource(&recovered_id, payload, None)
        .await
        .expect("send Resource over recovered Link");
    wait_for_resource_started(&seen, Duration::from_secs(15)).await;
    wait_for_outbound_resource_complete(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(30),
    )
    .await;
    wait_for_resource_digest_ack(
        &seen,
        256,
        &expected_digest,
        Duration::from_secs(30),
    )
    .await;

    drop(proxy);
    drop(transport);
}
