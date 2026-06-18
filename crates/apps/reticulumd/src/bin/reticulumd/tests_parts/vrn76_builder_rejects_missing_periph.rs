#[test]
fn vrn76_builder_rejects_missing_peripheral_id() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = vrn76_kiss_ble::build_config(&iface);
    assert!(result.is_err(), "missing peripheral_id should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("vrn76_kiss_ble.peripheral_id"));
}

#[test]
fn vrn76_builder_uses_profile_defaults_and_kiss_overrides() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        adapter: Some("Bluetooth".to_string()),
        mtu: Some(512),
        max_write_len: Some(128),
        preamble_ms: Some(410),
        tx_tail_ms: Some(30),
        persistence: Some(80),
        slot_time_ms: Some(40),
        kiss_flow_control: Some(true),
        scan_timeout_ms: Some(11_000),
        connect_timeout_ms: Some(4_000),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");
    assert_eq!(config.peripheral_id, "VR-N76");
    assert_eq!(config.adapter.as_deref(), Some("Bluetooth"));
    assert_eq!(config.transport.mtu, 512);
    assert_eq!(config.transport.max_write_len, 128);
    assert_eq!(config.transport.scan_timeout, Duration::from_millis(11_000));
    assert_eq!(config.transport.command_timeout, Duration::from_millis(4_000));
    assert_eq!(config.transport.kiss.preamble_ms, 410);
    assert_eq!(config.transport.kiss.tx_tail_ms, 30);
    assert_eq!(config.transport.kiss.persistence, 80);
    assert_eq!(config.transport.kiss.slot_time_ms, 40);
    assert!(config.transport.kiss.flow_control);
}

#[test]
fn vrn76_builder_carries_python_kiss_id_beacon_settings() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        id_callsign: Some("MYCALL-0".to_string()),
        id_interval: Some(600),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");

    assert_eq!(
        config.transport.kiss.id_beacon,
        Some(rns_transport::iface::kiss::KissIdBeaconConfig {
            callsign: b"MYCALL-0".to_vec(),
            interval: Duration::from_secs(600),
            min_payload_len: 15,
        })
    );
}

#[test]
fn vrn76_builder_preserves_python_empty_id_beacon_when_callsign_missing() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        id_interval: Some(600),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");

    assert_eq!(
        config.transport.kiss.id_beacon,
        Some(rns_transport::iface::kiss::KissIdBeaconConfig {
            callsign: Vec::new(),
            interval: Duration::from_secs(600),
            min_payload_len: 15,
        })
    );
}

#[test]
fn vrn76_builder_uses_raw_kiss_frame_mode() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        frame_mode: Some("raw_kiss".to_string()),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");
    assert_eq!(config.transport.frame_mode, Vrn76FrameMode::RawKiss);
}

#[test]
fn lora_startup_persists_state_file() {
    let temp = TempDir::new().expect("temp dir");
    let state_path = temp.path().join("lora-state.json");

    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("lora-main".to_string()),
        region: Some("US915".to_string()),
        state_path: Some(state_path.to_string_lossy().to_string()),
        ..InterfaceConfig::default()
    };

    lora::startup(&iface).expect("lora startup");
    let state = fs::read_to_string(&state_path).expect("state file exists");
    assert!(state.contains("\"version\": 1"));
}

#[test]
fn startup_status_metadata_is_embedded_in_interface_settings() {
    let mut record = InterfaceRecord {
        kind: "serial".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("serial-main".to_string()),
        settings: Some(json!({
            "device": "/dev/ttyUSB0",
            "baud_rate": 115200
        })),
    };

    mark_interface_startup_status(
        &mut record,
        "failed",
        Some("permission denied"),
        Some("deadbeef"),
    );

    let settings = record.settings.expect("settings should be present");
    let runtime = settings
        .get("_runtime")
        .and_then(|value| value.as_object())
        .expect("runtime metadata should be present");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert_eq!(
        runtime.get("startup_error").and_then(|value| value.as_str()),
        Some("permission denied")
    );
    assert_eq!(runtime.get("iface").and_then(|value| value.as_str()), Some("deadbeef"));
}

#[test]
fn runtime_status_metadata_is_embedded_in_interface_settings() {
    let mut record = InterfaceRecord {
        kind: "ble_gatt".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("ble-main".to_string()),
        settings: Some(json!({
            "peripheral_id": "AA:BB:CC:DD:EE:FF"
        })),
    };

    mark_interface_startup_status(&mut record, "spawned", None, Some("beefcafe"));
    mark_interface_runtime_fields(&mut record, "running", 0);

    let settings = record.settings.expect("settings should be present");
    let runtime = settings
        .get("_runtime")
        .and_then(|value| value.as_object())
        .expect("runtime metadata should be present");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));
    assert_eq!(runtime.get("reconnect_attempts").and_then(|value| value.as_u64()), Some(0));
    assert_eq!(runtime.get("iface").and_then(|value| value.as_str()), Some("beefcafe"));
}

#[test]
fn best_effort_startup_policy_allows_partial_failures() {
    let failures = vec![InterfaceStartupFailure {
        label: "lora-main".to_string(),
        kind: "lora".to_string(),
        error: "state marked uncertain".to_string(),
    }];
    enforce_startup_policy(false, &failures).expect("best-effort policy should not fail");
}

#[test]
fn strict_startup_policy_rejects_interface_failures() {
    let failures = vec![InterfaceStartupFailure {
        label: "lora-main".to_string(),
        kind: "lora".to_string(),
        error: "state marked uncertain".to_string(),
    }];
    let err = enforce_startup_policy(true, &failures).expect_err("strict policy should fail");
    assert!(err.contains("strict interface startup policy rejected"));
    assert!(err.contains("lora-main"));
}

#[test]
fn select_tcp_server_bind_uses_single_enabled_interface_when_transport_not_set() {
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![InterfaceConfig {
            kind: "tcp_server".to_string(),
            enabled: Some(true),
            host: None,
            port: Some(4242),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select server");
    assert_eq!(selected.bind_addr.as_deref(), Some("0.0.0.0:4242"));
    assert_eq!(selected.selected_index, Some(0));
}

#[test]
fn select_tcp_server_bind_prefers_transport_override() {
    let args = test_args(PathBuf::from("/tmp/db"), None, Some("127.0.0.1:4333".to_string()), false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("0.0.0.0".to_string()),
                port: Some(4242),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("127.0.0.1".to_string()),
                port: Some(4243),
                ..InterfaceConfig::default()
            },
        ],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("transport override wins");
    assert_eq!(selected.bind_addr.as_deref(), Some("127.0.0.1:4333"));
    assert_eq!(selected.selected_index, None);
}

#[test]
fn select_tcp_server_bind_rejects_multiple_enabled_servers_without_override() {
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("0.0.0.0".to_string()),
                port: Some(4242),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("127.0.0.1".to_string()),
                port: Some(4243),
                ..InterfaceConfig::default()
            },
        ],
    };

    let err = select_tcp_server_bind(&args, Some(&config)).expect_err("multiple servers must fail");
    assert!(err.contains("multiple enabled tcp_server interfaces"));
}

#[test]
fn bootstrap_best_effort_starts_configured_interfaces_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "/dev/ttyUSB0", baud_rate = 115200 }
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
    assert_eq!(interfaces.len(), 1);
    assert_eq!(
        interfaces[0]
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
}

#[test]
fn bootstrap_best_effort_starts_kiss_interface_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "kiss", enabled = true, name = "kiss-main", device = "__definitely_not_a_device__", baud_rate = 9600, kiss_flow_control = true }
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
    assert_eq!(interfaces.len(), 1);
    assert_eq!(
        interfaces[0]
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
}

#[test]
fn bootstrap_best_effort_starts_active_lora_interface_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let state_path = temp.path().join("lora-state.json");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "lora", enabled = true, name = "lora-main", region = "US915", state_path = "{}", device = "__definitely_not_a_device__", baud_rate = 115200, max_payload_bytes = 220 }}
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
    assert_eq!(interfaces.len(), 1);
    assert_eq!(
        interfaces[0]
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
    assert!(state_path.exists(), "active lora startup should still persist compliance state");
}
