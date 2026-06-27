#[cfg(not(feature = "vrn76-kiss-ble"))]
#[test]
fn bootstrap_best_effort_marks_vrn76_kiss_ble_feature_disabled_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, name = "vrn76-main", peripheral_id = "VR-N76" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let runtime = interfaces[0]
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert!(runtime
        .get("startup_error")
        .and_then(|value| value.as_str())
        .is_some_and(|error| error.contains("requires reticulumd feature vrn76-kiss-ble")));
}

#[cfg(not(feature = "rnode-ble"))]
#[test]
fn bootstrap_best_effort_marks_rnode_ble_feature_disabled_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let state_path = temp.path().join("lora-state.json");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "RNodeInterface", enabled = true, name = "rnode-ble", region = "US915", state_path = "{}", port = "ble://RNode 1234", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }}
]
"#,
            state_path.to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let runtime = interfaces[0]
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert!(runtime
        .get("startup_error")
        .and_then(|value| value.as_str())
        .is_some_and(|error| error.contains("requires reticulumd feature rnode-ble")));
}

#[test]
fn bootstrap_starts_tcp_server_from_config_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-main", host = "127.0.0.1", port = 0 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let tcp_server = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("tcp_server"))
        .expect("tcp_server entry");
    assert_eq!(
        tcp_server
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[test]
fn bootstrap_starts_backbone_listener_from_config_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "BackboneInterface", enabled = true, name = "backbone-main", listen_on = "127.0.0.1", port = 0 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let backbone = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("backbone"))
        .expect("backbone entry");
    assert_eq!(
        backbone
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
    assert_eq!(
        backbone
            .get("settings")
            .and_then(|value| value.get("mtu"))
            .and_then(|value| value.as_u64()),
        Some(1_048_576)
    );
}

#[test]
fn bootstrap_python_backbone_remote_alias_reports_backbone_client_status() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "BackboneInterface", enabled = true, name = "backbone-remote", remote = "127.0.0.1", port = 65535 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let backbone = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("backbone_client"))
        .expect("backbone_client entry");
    assert_eq!(backbone.get("host").and_then(|value| value.as_str()), Some("127.0.0.1"));
    assert_eq!(backbone.get("port").and_then(|value| value.as_u64()), Some(65535));
    let runtime = backbone
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("backbone client runtime");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert!(runtime.get("iface").and_then(|value| value.as_str()).is_some());
}

#[test]
fn bootstrap_python_tcp_client_kiss_framing_alias_reports_kiss_tcp_status() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-kiss-tcp", target_host = "127.0.0.1", target_port = 65535, kiss_framing = true, flow_control = true, fixed_mtu = 512 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let kiss_tcp = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("kiss_tcp_client"))
        .expect("kiss_tcp_client entry");
    let runtime = kiss_tcp
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("kiss tcp runtime");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert!(runtime.get("iface").and_then(|value| value.as_str()).is_some());
    let status = &runtime["kiss_tcp"]["status"];
    assert_eq!(status["bearer"].as_str(), Some("tcp"));
    assert_eq!(status["endpoint"].as_str(), Some("127.0.0.1:65535"));
    assert_eq!(status["kiss_flow_control"].as_bool(), Some(true));
    assert_eq!(status["mtu"].as_u64(), Some(512));
}

#[test]
fn bootstrap_python_rnode_alias_reports_lora_rnode_status() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let state_path = temp.path().join("lora-state.json");
    let device_path = temp.path().join("not-a-real-rnode-serial");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "RNodeInterface", enabled = true, name = "python-rnode", region = "US915", state_path = "{}", port = "{}", baud_rate = 115200, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }}
]
"#,
            state_path.to_string_lossy().replace('\\', "\\\\"),
            device_path.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();
    let context = runtime.block_on(local.run_until(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    }));
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let lora = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("lora"))
        .unwrap_or_else(|| panic!("lora entry, interfaces={interfaces:?}"));
    let runtime = lora
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("lora runtime");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert!(runtime.get("iface").and_then(|value| value.as_str()).is_some());
    let rnode_status = &runtime["lora"]["rnode_status"];
    assert_eq!(
        rnode_status.get("endpoint").and_then(|value| value.as_str()),
        Some(device_path.to_string_lossy().as_ref())
    );
    assert_eq!(rnode_status.get("bearer").and_then(|value| value.as_str()), Some("serial"));
    assert_eq!(rnode_status.get("baud_rate").and_then(|value| value.as_u64()), Some(115_200));
    assert_eq!(
        rnode_status["configured"]["frequency_hz"].as_u64(),
        Some(915_000_000)
    );
    assert_eq!(rnode_status["configured"]["spreading_factor"].as_u64(), Some(9));
}

#[test]
fn bootstrap_starts_pipe_interface_from_config() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "PipeInterface", enabled = true, name = "pipe-main", command = "cat", respawn_delay = 0.1 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let pipe = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("pipe"))
        .expect("pipe entry");
    assert_eq!(
        pipe.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
    assert_eq!(
        pipe.get("settings")
            .and_then(|value| value.get("mtu"))
            .and_then(|value| value.as_u64()),
        Some(1_064)
    );
    let pipe_status = &pipe["settings"]["_runtime"]["pipe"]["status"];
    assert_eq!(pipe_status["command"].as_str(), Some("cat"));
    assert_eq!(pipe_status["process_state"].as_str(), Some("configured"));
    assert_eq!(pipe_status["pipe_is_open"].as_bool(), Some(false));
    assert_eq!(pipe_status["respawn_attempts"].as_u64(), Some(0));
    assert!(pipe_status["last_error"].is_null());
}

#[test]
fn bootstrap_starts_local_interface_from_config_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "LocalInterface", enabled = true, name = "local-main", shared_instance_type = "tcp", shared_instance_port = 0 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local"))
        .expect("local entry");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[test]
fn bootstrap_starts_reticulum_global_shared_instance_without_local_interface_entry() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
[reticulum]
share_instance = true
shared_instance_type = "tcp"
shared_instance_port = 0
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local"))
        .expect("implicit local entry");
    assert_eq!(local.get("name").and_then(|value| value.as_str()), Some("shared-instance"));
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[test]
fn bootstrap_starts_implicit_shared_local_tcp_beside_configured_tcp_server() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "TCPServerInterface", enabled = true, name = "tcp-main", host = "127.0.0.1", port = 0 }
]

[reticulum]
share_instance = true
shared_instance_type = "tcp"
shared_instance_port = 0
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let tcp_server = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("tcp_server"))
        .expect("tcp server entry");
    assert_eq!(tcp_server.get("name").and_then(|value| value.as_str()), Some("tcp-main"));
    assert_eq!(
        tcp_server
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );

    let local = interfaces
        .iter()
        .find(|entry| {
            entry.get("type").and_then(|value| value.as_str()) == Some("local")
                && entry.get("name").and_then(|value| value.as_str()) == Some("shared-instance")
        })
        .expect("implicit local entry");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[test]
fn bootstrap_reticulum_global_share_instance_false_does_not_start_implicit_local() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
[reticulum]
share_instance = false
shared_instance_type = "tcp"
shared_instance_port = 0
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    assert!(
        interfaces
            .iter()
            .all(|entry| entry.get("type").and_then(|value| value.as_str()) != Some("local")),
        "share_instance=false should not synthesize local interface: {interfaces:?}"
    );
}

#[test]
fn bootstrap_attaches_local_interface_when_shared_instance_is_already_listening() {
    let shared_listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind shared local instance");
    let port = shared_listener.local_addr().expect("shared local addr").port();
    shared_listener.set_nonblocking(true).expect("nonblocking listener");

    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "LocalInterface", enabled = true, name = "local-attach", shared_instance_type = "tcp", shared_instance_port = {port} }}
]
"#
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        let context =
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
                .await;
        tokio::task::yield_now().await;
        context
    });

    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local"))
        .expect("local entry");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("attached")
    );
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("port"))
            .and_then(|value| value.as_u64()),
        Some(u64::from(port))
    );
}

#[test]
fn bootstrap_local_client_interface_forces_tcp_attach() {
    let shared_listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind shared local instance");
    let port = shared_listener.local_addr().expect("shared local addr").port();
    shared_listener.set_nonblocking(true).expect("nonblocking listener");

    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "LocalClientInterface", enabled = true, name = "local-client", shared_instance_type = "tcp", shared_instance_port = {port} }}
]
"#
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        let context =
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
                .await;
        tokio::task::yield_now().await;
        context
    });

    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local_client"))
        .expect("local_client entry");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("attached")
    );
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("port"))
            .and_then(|value| value.as_u64()),
        Some(u64::from(port))
    );
}

#[cfg(unix)]
#[test]
fn bootstrap_starts_local_unix_interface_from_config_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let socket_path = temp.path().join("reticulum.sock");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "LocalInterface", enabled = true, name = "local-unix", shared_instance_type = "unix", socket_path = "{}" }}
]
"#,
            socket_path.display()
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        let context =
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
                .await;
        tokio::task::yield_now().await;
        context
    });
    assert!(socket_path.exists(), "local unix listener should create socket path");

    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local"))
        .expect("local entry");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("shared_instance_type"))
            .and_then(|value| value.as_str()),
        Some("unix")
    );
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("socket_path"))
            .and_then(|value| value.as_str()),
        socket_path.to_str()
    );
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn bootstrap_starts_local_unix_abstract_interface_from_instance_name() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let instance_name = format!(
        "codex-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    );
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "LocalInterface", enabled = true, name = "local-unix-abstract", shared_instance_type = "unix", instance_name = "{instance_name}" }}
]
"#
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        let context =
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
                .await;
        tokio::task::yield_now().await;
        context
    });

    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local"))
        .expect("local entry");
    let expected_socket_path = format!("@rns/{instance_name}");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("socket_path"))
            .and_then(|value| value.as_str()),
        Some(expected_socket_path.as_str())
    );
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn bootstrap_attaches_local_unix_when_abstract_instance_is_already_listening() {
    fn abstract_socket_path(name: &str) -> PathBuf {
        let mut bytes = Vec::with_capacity(name.len() + 1);
        bytes.push(0);
        bytes.extend_from_slice(name.as_bytes());
        PathBuf::from(OsString::from_vec(bytes))
    }

    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let instance_name = format!(
        "codex-attach-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    );
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "LocalInterface", enabled = true, name = "local-unix-attach", shared_instance_type = "unix", instance_name = "{instance_name}" }}
]
"#
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        let abstract_name = format!("rns/{instance_name}");
        let _shared_listener = tokio::net::UnixListener::bind(abstract_socket_path(
            abstract_name.as_str(),
        ))
        .expect("bind shared abstract local instance");
        let context =
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
                .await;
        tokio::task::yield_now().await;
        context
    });

    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let local = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("local"))
        .expect("local entry");
    let expected_socket_path = format!("@rns/{instance_name}");
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("attached")
    );
    assert_eq!(
        local
            .get("settings")
            .and_then(|value| value.get("socket_path"))
            .and_then(|value| value.as_str()),
        Some(expected_socket_path.as_str())
    );
}

#[test]
fn bootstrap_transport_override_shadows_configured_tcp_servers_without_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-a", host = "127.0.0.1", port = 4242 },
  { type = "tcp_server", enabled = true, name = "server-b", host = "127.0.0.1", port = 4243 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            true,
        ))
        .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let shadowed = interfaces
        .iter()
        .filter(|entry| {
            entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("shadowed_by_transport_override")
        })
        .count();
    assert!(shadowed >= 2);
}

#[test]
fn bootstrap_transport_override_shadows_missing_port_tcp_server_without_strict_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-a", host = "127.0.0.1", port = 4242 },
  { type = "tcp_server", enabled = true, name = "server-b" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            true,
        ))
        .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let shadowed_missing_port = interfaces.iter().any(|entry| {
        entry.get("name").and_then(|value| value.as_str()) == Some("server-b")
            && entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("shadowed_by_transport_override")
    });

    assert!(
        shadowed_missing_port,
        "shadowed tcp_server without a port should remain non-fatal under transport override"
    );
}

#[test]
fn reticulum_parity_matrix_mentions_config_driven_lxmd_tcp_server_startup() {
    let parity_matrix_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/status/reticulum-parity-matrix.md");
    let text = fs::read_to_string(&parity_matrix_path).expect("read reticulum parity matrix");

    assert!(
        text.contains("Python-style interface-driven `tcp_server` startup now works from config")
            && text.contains("without Rust-only transport overrides"),
        "reticulum parity matrix should document config-driven lxmd tcp_server startup parity"
    );
}

#[test]
fn kiss_docs_document_bearers_and_vtn76_bluetooth() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let kiss_runbook =
        fs::read_to_string(repo_root.join("docs/runbooks/reticulumd-kiss-interface.md"))
            .expect("read KISS runbook");
    let vrn76_interface = fs::read_to_string(repo_root.join("docs/interfaces/vrn76-kiss-ble.md"))
        .expect("read VR-N76 KISS BLE interface doc");

    assert!(
        kiss_runbook.contains("serial, Bluetooth, Wi-Fi/TCP"),
        "KISS runbook should document the supported connection bearers"
    );
    assert!(
        vrn76_interface.contains("VT-N76/VR-N76")
            && vrn76_interface.contains("Bluetooth KISS operation"),
        "VR-N76 interface doc should state that VT-N76/VR-N76 KISS uses Bluetooth"
    );
    assert!(
        vrn76_interface.contains("Host Bluetooth Boundary")
            && vrn76_interface.contains("outside this repository")
            && vrn76_interface.contains("adapter drivers")
            && vrn76_interface.contains("pairing or bonding"),
        "VR-N76 interface doc should separate repo-owned KISS/Benshi logic from OS Bluetooth setup"
    );
}

#[test]
fn android_ble_native_target_gates_include_android() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulumd_manifest =
        fs::read_to_string(repo_root.join("crates/apps/reticulumd/Cargo.toml"))
            .expect("read reticulumd manifest");
    let rns_tools_manifest = fs::read_to_string(repo_root.join("crates/apps/rns-tools/Cargo.toml"))
        .expect("read rns-tools manifest");
    let ble_mod = fs::read_to_string(
        repo_root.join("crates/apps/reticulumd/src/bin/reticulumd/interfaces/ble/mod.rs"),
    )
    .expect("read reticulumd BLE module");
    let rnx_ble = fs::read_to_string(repo_root.join("crates/apps/rns-tools/src/bin/rnx/ble.rs"))
        .expect("read rns-tools BLE commands");

    for (label, text) in [
        ("reticulumd target dependencies", reticulumd_manifest.as_str()),
        ("rns-tools target dependencies", rns_tools_manifest.as_str()),
        ("reticulumd BLE dispatch", ble_mod.as_str()),
        ("rns-tools BLE commands", rnx_ble.as_str()),
    ] {
        assert!(
            text.contains("target_os = \"android\""),
            "{label} should include android in native BLE target gates"
        );
    }
}

#[test]
fn bootstrap_starts_udp_interface_from_config() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "udp-main", host = "127.0.0.1", port = 0, target_host = "127.0.0.1", target_port = 4242 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let udp = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("udp"))
        .expect("udp entry");
    assert_eq!(udp.get("host").and_then(|value| value.as_str()), Some("127.0.0.1"));
    assert_eq!(udp.get("port").and_then(|value| value.as_u64()), Some(0));
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("target_host"))
            .and_then(|value| value.as_str()),
        Some("127.0.0.1")
    );
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("target_port"))
            .and_then(|value| value.as_u64()),
        Some(4242)
    );
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
}
