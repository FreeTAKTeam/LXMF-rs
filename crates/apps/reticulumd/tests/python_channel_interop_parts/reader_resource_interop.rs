/// Exercises the reader-backed sender against the pinned Python Resource
/// receiver, including a split transfer and an exact cross-implementation
/// digest acknowledgement.
#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn rust_reader_to_python_split_resource_roundtrip() {
    let _interop_guard = python_interop_guard().await;
    let paths = python_channel_interop_paths();

    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-reader-resource");
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
    let mut config = TransportConfig::new("python-reader-resource-interop", &rust_identity, true);
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

    let payload = rust_resource_fixture(MAX_EFFICIENT_SIZE + 257);
    let expected_digest = digest_hex(&payload);
    let metadata = rmp_serde::to_vec(&String::from("rust-reader-meta")).expect("metadata");
    let payload_path = temp.path().join("reader-resource.bin");
    fs::write(&payload_path, &payload).expect("write reader-backed resource fixture");
    let reader = fs::File::open(&payload_path).expect("open reader-backed resource fixture");
    let mut resource_events = transport.resource_events();
    let resource_hash = transport
        .send_resource_from_reader(&link_id, reader, payload.len() as u64, Some(metadata))
        .await
        .expect("send file-backed resource");
    wait_for_outbound_resource_complete(
        &mut resource_events,
        resource_hash,
        Duration::from_secs(30),
    )
    .await;
    wait_for_resource_digest_ack(
        &seen,
        payload.len(),
        &expected_digest,
        Duration::from_secs(30),
    )
    .await;
}
