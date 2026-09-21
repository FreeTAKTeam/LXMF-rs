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
        wait_for_outbound_resource_failed(
            &mut resource_events,
            resource_hash,
            Duration::from_secs(45),
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
            Duration::from_secs(15),
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
