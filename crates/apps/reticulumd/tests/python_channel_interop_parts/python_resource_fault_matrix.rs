use python_resource_fault_proxy::{PythonResourceFaultProxy, ResourceFaultMode};

async fn run_python_resource_fault(mode: ResourceFaultMode, expect_failure: bool) {
    let paths = python_channel_interop_paths();
    let server_port = free_tcp_port();
    let temp = tempfile::tempdir().expect("tempdir");
    let py_config_dir = temp.path().join("python-rns-resource-fault-client");
    fs::create_dir_all(&py_config_dir).expect("python config dir");

    let proxy = PythonResourceFaultProxy::bind(server_port, mode).await;
    write_python_client_config(&py_config_dir, proxy.port());

    let rust_identity = PrivateIdentity::new_from_rand(OsRng);
    let rust_identity = to_transport_private_identity(&rust_identity);
    let mut config = TransportConfig::new("python-resource-fault-rust-server", &rust_identity, true);
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
    let resource_size = MAX_EFFICIENT_SIZE + 257;
    let child = paths.spawn_resource_client(&py_config_dir, &destination_hash, resource_size, 45.0);
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

    if expect_failure {
        let reason =
            wait_for_inbound_resource_failure(&mut resource_events, link_id, Duration::from_secs(45))
                .await;
        assert!(!reason.is_empty(), "missing Resource failure reason");
    } else {
        let complete =
            wait_for_inbound_resource_data(&mut resource_events, link_id, Duration::from_secs(30))
                .await;
        assert_eq!(complete.data.len(), resource_size);
        assert_eq!(
            complete.metadata.as_deref().and_then(|metadata| {
                rmp_serde::from_slice::<String>(metadata).ok()
            }),
            Some("python-meta".to_string())
        );
    }

    let child = guard.child.take().expect("python resource client");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join Python resource client")
        .expect("wait for Python resource client");
    if expect_failure {
        assert!(
            !output.status.success(),
            "Python client unexpectedly reported a successful missing-fragment transfer\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        assert!(
            output.status.success(),
            "Python resource client failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("\"resource\": \"complete\""),
            "Python client did not report Resource completion: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    drop(proxy);
    drop(transport);
}

#[tokio::test]
#[ignore = "requires local Python Reticulum checkout"]
async fn pinned_python_resource_fault_matrix() {
    let _interop_guard = python_interop_guard().await;

    run_python_resource_fault(ResourceFaultMode::DropFirst, false).await;
    run_python_resource_fault(ResourceFaultMode::DuplicateFirst, false).await;
    run_python_resource_fault(ResourceFaultMode::ReorderFirstTwo, false).await;
    run_python_resource_fault(ResourceFaultMode::DropAll, true).await;
}

