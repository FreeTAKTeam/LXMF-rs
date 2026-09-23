fn cancellation_payload(size: usize) -> Vec<u8> {
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

struct FaultingResourceReader {
    data: Vec<u8>,
    offset: usize,
    fail_at: usize,
}

impl std::io::Read for FaultingResourceReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.fail_at {
            return Err(std::io::Error::other("synthetic mixed-peer reader failure"));
        }
        let length = (self.fail_at - self.offset).min(buffer.len());
        buffer[..length].copy_from_slice(&self.data[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_sender_maps_pinned_python_receiver_cancel_to_rejection() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-cancel-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "cancel-resource");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-cancel-rust-sender", &rust_identity, true);
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
    let payload = cancellation_payload(MAX_EFFICIENT_SIZE * 2 + 257);
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource_from_reader(
            &link_id,
            std::io::Cursor::new(payload.clone()),
            payload.len() as u64,
            None,
        )
        .await
        .expect("send cancellable reader-backed resource");

    wait_for_outbound_resource_rejected(&mut resource_events, resource_hash, Duration::from_secs(15))
        .await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_sender_reports_pinned_python_receiver_shutdown() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-shutdown-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource-shutdown");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let mut guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-resource-shutdown-rust-sender", &rust_identity, true);
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
        .expect("register resource shutdown acknowledgement handler");

    let payload = cancellation_payload(MAX_EFFICIENT_SIZE * 2 + 257);
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource_with_compression(&link_id, payload, None, false)
        .await
        .expect("send resource before peer shutdown");
    wait_for_resource_started(&seen, Duration::from_secs(15)).await;

    let mut child = guard.child.take().expect("python endpoint child");
    child.kill().expect("kill Python receiver");
    child.wait().expect("wait for Python receiver shutdown");

    wait_for_outbound_resource_failed(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(30),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_reader_reports_pinned_python_reader_failure() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-reader-failure-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource-reader-failure");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-reader-failure-rust-sender", &rust_identity, true);
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
        .expect("register reader failure acknowledgement handler");

    let payload = cancellation_payload(MAX_EFFICIENT_SIZE + 257);
    let reader = FaultingResourceReader {
        data: payload.clone(),
        offset: 0,
        fail_at: MAX_EFFICIENT_SIZE,
    };
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource_from_reader(&link_id, reader, payload.len() as u64, None)
        .await
        .expect("send reader-backed resource before injected failure");
    wait_for_resource_started(&seen, Duration::from_secs(15)).await;

    wait_for_outbound_resource_failed(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(15),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_file_reader_reports_pinned_python_file_truncation() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-file-reader-failure-server");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_config(&py_config_dir, server_port);

    let mut child = paths.spawn_endpoint(&py_config_dir, "resource-reader-failure");
    let ready = read_ready(&mut child).expect("python endpoint ready");
    let _guard = ChildGuard { child: Some(child) };
    wait_for_port(server_port, Duration::from_secs(5)).await;

    let target_hash =
        AddressHash::new_from_hex_string(&ready.destination_hash).expect("destination hash");
    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-file-reader-failure-rust-sender", &rust_identity, true);
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
        .expect("register file reader failure acknowledgement handler");

    let resource_size = MAX_EFFICIENT_SIZE * 2 + 257;
    let payload_path = temp.path().join("truncated-resource.bin");
    fs::write(&payload_path, cancellation_payload(resource_size))
        .expect("write file-backed failure fixture");
    let reader = fs::File::open(&payload_path).expect("open file-backed failure fixture");
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource_from_reader(&link_id, reader, resource_size as u64, None)
        .await
        .expect("send file-backed resource before truncation");
    wait_for_resource_started(&seen, Duration::from_secs(15)).await;

    let truncated = fs::OpenOptions::new()
        .write(true)
        .open(&payload_path)
        .expect("reopen file-backed failure fixture");
    truncated
        .set_len(MAX_EFFICIENT_SIZE as u64)
        .expect("truncate file-backed failure fixture");

    wait_for_outbound_resource_failed(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(20),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_receiver_reports_pinned_python_sender_cancellation() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-cancel-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-cancel-rust-receiver", &rust_identity, true);
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

    let child = paths.spawn_cancel_resource_client(
        &py_config_dir,
        &destination_hash,
        MAX_EFFICIENT_SIZE * 2 + 257,
        30.0,
    );
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
    let reason =
        wait_for_inbound_resource_failure(&mut resource_events, link_id, Duration::from_secs(15))
            .await;
    assert_eq!(reason, "remote_cancelled");

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python child")
        .expect("wait for python child");
    assert!(
        output.status.success(),
        "python cancellation client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("\"cancelled\""),
        "python cancellation client did not report its callback status"
    );
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_receiver_reports_pinned_python_file_reader_failure() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-file-reader-failure-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");
    write_python_client_config(&py_config_dir, server_port);

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config =
        TransportConfig::new("python-file-reader-failure-rust-receiver", &rust_identity, true);
    config.set_path_request_timeout_secs(2);
    config.set_resource_retry_interval_secs(1);
    config.set_resource_retry_limit(4);
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

    let child = paths.spawn_faulting_resource_client(
        &py_config_dir,
        &destination_hash,
        MAX_EFFICIENT_SIZE * 2 + 257,
        12.0,
    );
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
    let reason =
        wait_for_inbound_resource_failure(&mut resource_events, link_id, Duration::from_secs(20))
            .await;
    assert!(
        matches!(reason.as_str(), "retry_limit_exhausted" | "link_closed"),
        "unexpected terminal reason after Python reader failure: {reason}"
    );

    let child = guard.child.take().expect("python child");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join python child")
        .expect("wait for python child");
    assert!(
        !output.status.success(),
        "python file-reader failure client unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("synthetic Python file-reader failure"),
        "python file-reader exception was not observed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
    assert!(
        stderr.contains("timed out waiting for resource"),
        "python sender did not fail closed after its reader exception\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
}
