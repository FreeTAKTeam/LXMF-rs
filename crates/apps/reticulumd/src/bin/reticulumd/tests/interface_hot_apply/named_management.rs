use super::super::named_management as management;
use super::*;

fn available_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
    listener.local_addr().expect("local address").port()
}

#[tokio::test(flavor = "current_thread")]
async fn named_management_attaches_reloads_and_detaches_real_tcp_listener() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let weak = Arc::downgrade(&daemon);
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let refreshes = runtime_refreshes();
    let mut managed = HashMap::new();
    let first_port = available_port();
    let second_port = available_port();
    let mut disabled = tcp_server_record("named-listener", "127.0.0.1", first_port);
    disabled.enabled = false;
    daemon.replace_interfaces(vec![disabled]);

    let attached = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "attach",
            "named-listener",
            Some(tcp_server_record("named-listener", "127.0.0.1", first_port)),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect("attach after listener bind");
    assert_eq!(attached["complete"], true);
    assert!(TcpStream::connect(("127.0.0.1", first_port)).await.is_ok());
    let duplicate = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "attach",
            "named-listener",
            Some(tcp_server_record("named-listener", "127.0.0.1", first_port)),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect_err("duplicate attach");
    assert_eq!(duplicate.kind(), io::ErrorKind::AlreadyExists);

    let reloaded = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "reload",
            "named-listener",
            Some(tcp_server_record("named-listener", "127.0.0.1", second_port)),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect("reload after replacement bind");
    assert_eq!(reloaded["complete"], true);
    assert!(TcpStream::connect(("127.0.0.1", first_port)).await.is_err());
    let accepted_peer =
        TcpStream::connect(("127.0.0.1", second_port)).await.expect("connect accepted child");
    for _ in 0..40 {
        if manager.lock().await.interface_hashes().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(manager.lock().await.interface_hashes().len(), 2);

    let detached = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new("detach", "named-listener", None),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect("detach after listener stop");
    assert_eq!(detached["state"], "detached");
    assert!(managed.is_empty());
    assert!(manager.lock().await.interface_hashes().is_empty());
    assert!(TcpStream::connect(("127.0.0.1", second_port)).await.is_err());
    assert!(!daemon.interface_records()[0].enabled);
    drop(accepted_peer);
}

#[tokio::test(flavor = "current_thread")]
async fn named_management_failed_reload_restores_previous_listener() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let weak = Arc::downgrade(&daemon);
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let refreshes = runtime_refreshes();
    let mut managed = HashMap::new();
    let working_port = available_port();
    daemon.replace_interfaces(vec![]);
    management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "attach",
            "named-listener",
            Some(tcp_server_record("named-listener", "127.0.0.1", working_port)),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect("initial attach");

    let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupy replacement port");
    let occupied_port = occupied.local_addr().expect("occupied address").port();
    let error = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "reload",
            "named-listener",
            Some(tcp_server_record("named-listener", "127.0.0.1", occupied_port)),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect_err("occupied replacement must fail");
    assert!(error.to_string().contains("previous interface restored"), "{error}");
    assert!(TcpStream::connect(("127.0.0.1", working_port)).await.is_ok());
    assert_eq!(daemon.interface_records()[0].port, Some(working_port));
}

#[tokio::test(flavor = "current_thread")]
async fn named_management_rejects_missing_and_unsupported_names() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let weak = Arc::downgrade(&daemon);
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let refreshes = runtime_refreshes();
    let mut managed = HashMap::new();
    let missing = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new("detach", "missing", None),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect_err("missing interface");
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);

    daemon.replace_interfaces(vec![InterfaceRecord {
        kind: "auto".to_string(),
        name: Some("protected".to_string()),
        enabled: true,
        host: None,
        port: None,
        settings: None,
    }]);
    let unsupported = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new("detach", "protected", None),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect_err("unsupported interface");
    assert_eq!(unsupported.kind(), io::ErrorKind::Unsupported);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn named_management_rpc_waits_for_real_attach_and_detach() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let temp = tempfile::tempdir().expect("temporary config directory");
    let config_path = temp.path().join("reticulum.toml");
    let port = available_port();
    std::fs::write(
        &config_path,
        format!(
            "interfaces = [{{ type = \"tcp_server\", enabled = false, name = \"named-listener\", host = \"127.0.0.1\", port = {port} }}]\n"
        ),
    )
    .expect("write temporary config");
    let mut disabled = tcp_server_record("named-listener", "127.0.0.1", port);
    disabled.enabled = false;
    daemon.replace_interfaces(vec![disabled]);
    let bridge =
        InterfaceHotApplyBridge::spawn_with_daemon(manager, Vec::new(), Arc::downgrade(&daemon))
            .with_management_config(Some(config_path), true);
    daemon.set_interface_mutation_bridge(Arc::new(bridge));

    for (operation, expected_state) in [("attach", "active"), ("detach", "detached")] {
        let daemon = Arc::clone(&daemon);
        let response = tokio::task::spawn_blocking(move || {
            daemon.handle_rpc(RpcRequest {
                id: 155,
                method: "manage_interface".to_string(),
                params: Some(json!({ "operation": operation, "name": "named-listener" })),
            })
        })
        .await
        .expect("RPC dispatcher task")
        .expect("RPC dispatch");
        assert!(response.error.is_none(), "RPC error: {:?}", response.error);
        assert_eq!(response.result.expect("management result")["state"], expected_state);
        assert_eq!(TcpStream::connect(("127.0.0.1", port)).await.is_ok(), operation == "attach");
    }
}

#[test]
fn named_management_bridge_honors_disabled_policy_before_queueing() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let mut bridge = test_bridge(tx);
    bridge.management_enabled = false;
    let error = bridge.manage_named_interface("attach", "uplink").expect_err("policy disabled");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(rx.try_recv().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn named_management_failed_startup_leaves_interface_detached() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let weak = Arc::downgrade(&daemon);
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let refreshes = runtime_refreshes();
    let mut managed = HashMap::new();
    let occupied = TcpListener::bind("127.0.0.1:0").await.expect("occupy listener port");
    let port = occupied.local_addr().expect("occupied address").port();
    let mut disabled = tcp_server_record("busy", "127.0.0.1", port);
    disabled.enabled = false;
    daemon.replace_interfaces(vec![disabled]);
    let error = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "attach",
            "busy",
            Some(tcp_server_record("busy", "127.0.0.1", port)),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect_err("occupied bind must fail");
    assert!(error.to_string().contains("bind_error"), "{error}");
    assert!(managed.is_empty());
    assert!(!daemon.interface_records()[0].enabled);

    daemon.replace_interfaces(vec![]);
    let error = management::apply(
        &manager,
        &mut managed,
        management::NamedInterfaceAction::new(
            "attach",
            "broken-pipe",
            Some(pipe_record("broken-pipe", "/definitely/missing/lxmf-pipe-command")),
        ),
        None,
        &refreshes,
        Some(&weak),
    )
    .await
    .expect_err("unspawnable pipe must fail");
    assert!(error.to_string().contains("respawning"), "{error}");
    assert!(managed.is_empty());
    assert!(daemon.interface_records().is_empty());
}
