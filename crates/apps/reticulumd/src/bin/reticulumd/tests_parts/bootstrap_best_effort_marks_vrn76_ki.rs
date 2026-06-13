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
