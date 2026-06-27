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
fn pipe_builder_uses_python_defaults_and_overrides() {
    let iface = InterfaceConfig {
        kind: "pipe".to_string(),
        enabled: Some(true),
        command: Some("cat".to_string()),
        respawn_delay: Some(0.25),
        mtu: Some(512),
        ..InterfaceConfig::default()
    };

    let adapter = pipe::build_adapter(&iface).expect("build pipe adapter");

    assert_eq!(adapter.command(), "cat");
    assert_eq!(adapter.mtu_value(), 512);
}

#[test]
fn pipe_builder_rejects_missing_command() {
    let iface = InterfaceConfig {
        kind: "pipe".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };

    let err = match pipe::build_adapter(&iface) {
        Ok(_) => panic!("missing command should fail"),
        Err(err) => err,
    };
    assert!(err.contains("pipe.command"));
}

#[test]
fn rnode_multi_builder_carries_unpadded_python_id_beacon_settings() {
    let iface = InterfaceConfig {
        kind: "rnode_multi".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyACM0".to_string()),
        baud_rate: Some(115_200),
        id_callsign: Some("MYCALL-0".to_string()),
        id_interval: Some(600),
        extra: [(
            "radio0".to_string(),
            toml::Value::Table(
                [
                    ("vport".to_string(), toml::Value::Integer(2)),
                    ("frequency".to_string(), toml::Value::Integer(915_000_000)),
                    ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                    ("spreadingfactor".to_string(), toml::Value::Integer(9)),
                    ("codingrate".to_string(), toml::Value::Integer(5)),
                    ("txpower".to_string(), toml::Value::Integer(17)),
                ]
                .into_iter()
                .collect(),
            ),
        )]
        .into_iter()
        .collect(),
        ..InterfaceConfig::default()
    };
    let manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        rns_transport::iface::InterfaceManager::new(8),
    ));

    let adapter = rnode_multi::build_adapter(&iface, manager).expect("build rnode multi adapter");
    let beacon = adapter.id_beacon().expect("rnode multi id beacon");

    assert_eq!(adapter.mtu_value(), 508);
    assert_eq!(beacon.callsign, b"MYCALL-0");
    assert_eq!(beacon.interval, std::time::Duration::from_secs(600));
    assert_eq!(beacon.min_payload_len, 0);
}

#[test]
fn rnode_multi_builder_accepts_tcp_endpoint_without_serial_baud_rate() {
    let iface = InterfaceConfig {
        kind: "rnode_multi".to_string(),
        enabled: Some(true),
        device: Some("tcp://192.0.2.10:8001".to_string()),
        extra: [(
            "radio0".to_string(),
            toml::Value::Table(
                [
                    ("vport".to_string(), toml::Value::Integer(2)),
                    ("frequency".to_string(), toml::Value::Integer(915_000_000)),
                    ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                    ("spreadingfactor".to_string(), toml::Value::Integer(9)),
                    ("codingrate".to_string(), toml::Value::Integer(5)),
                    ("txpower".to_string(), toml::Value::Integer(17)),
                ]
                .into_iter()
                .collect(),
            ),
        )]
        .into_iter()
        .collect(),
        ..InterfaceConfig::default()
    };
    let manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        rns_transport::iface::InterfaceManager::new(8),
    ));

    let adapter = rnode_multi::build_adapter(&iface, manager).expect("build tcp rnode multi");

    assert_eq!(adapter.endpoint(), "192.0.2.10:8001");
    assert_eq!(adapter.baud_rate(), None);
    assert_eq!(adapter.subinterfaces().len(), 1);
}

#[test]
fn rnode_multi_builder_applies_explicit_mtu() {
    let iface = InterfaceConfig {
        kind: "rnode_multi".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyACM0".to_string()),
        baud_rate: Some(115_200),
        mtu: Some(1024),
        extra: [(
            "radio0".to_string(),
            toml::Value::Table(
                [
                    ("vport".to_string(), toml::Value::Integer(2)),
                    ("frequency".to_string(), toml::Value::Integer(915_000_000)),
                    ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                    ("spreadingfactor".to_string(), toml::Value::Integer(9)),
                    ("codingrate".to_string(), toml::Value::Integer(5)),
                    ("txpower".to_string(), toml::Value::Integer(17)),
                ]
                .into_iter()
                .collect(),
            ),
        )]
        .into_iter()
        .collect(),
        ..InterfaceConfig::default()
    };
    let manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        rns_transport::iface::InterfaceManager::new(8),
    ));

    let adapter = rnode_multi::build_adapter(&iface, manager).expect("build rnode multi adapter");

    assert_eq!(adapter.mtu_value(), 1024);
}

#[test]
fn rnode_multi_builder_uses_interface_enabled_for_child_radios() {
    let iface = InterfaceConfig {
        kind: "rnode_multi".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyACM0".to_string()),
        baud_rate: Some(115_200),
        extra: [
            (
                "radio0".to_string(),
                toml::Value::Table(
                    [
                        ("enabled".to_string(), toml::Value::Boolean(false)),
                        ("interface_enabled".to_string(), toml::Value::Boolean(true)),
                        ("vport".to_string(), toml::Value::Integer(2)),
                        ("frequency".to_string(), toml::Value::Integer(915_000_000)),
                        ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                        ("spreadingfactor".to_string(), toml::Value::Integer(9)),
                        ("codingrate".to_string(), toml::Value::Integer(5)),
                        ("txpower".to_string(), toml::Value::Integer(-9)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                "radio1".to_string(),
                toml::Value::Table(
                    [
                        ("interface_enabled".to_string(), toml::Value::Boolean(false)),
                        ("enabled".to_string(), toml::Value::Boolean(true)),
                        ("vport".to_string(), toml::Value::Integer(3)),
                        ("frequency".to_string(), toml::Value::Integer(920_000_000)),
                        ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                        ("spreadingfactor".to_string(), toml::Value::Integer(10)),
                        ("codingrate".to_string(), toml::Value::Integer(5)),
                        ("txpower".to_string(), toml::Value::Integer(14)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
        ..InterfaceConfig::default()
    };
    let manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        rns_transport::iface::InterfaceManager::new(8),
    ));

    let adapter = rnode_multi::build_adapter(&iface, manager).expect("build rnode multi adapter");

    assert_eq!(adapter.subinterfaces().len(), 1);
    assert_eq!(adapter.subinterfaces()[0].vport, 2);
    assert_eq!(adapter.subinterfaces()[0].config.tx_power_dbm, -9);
}

#[test]
fn rnode_multi_builder_uses_enabled_for_child_radios_when_interface_enabled_absent() {
    let iface = InterfaceConfig {
        kind: "rnode_multi".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyACM0".to_string()),
        baud_rate: Some(115_200),
        extra: [
            (
                "radio0".to_string(),
                toml::Value::Table(
                    [
                        ("enabled".to_string(), toml::Value::Boolean(false)),
                        ("vport".to_string(), toml::Value::Integer(2)),
                        ("frequency".to_string(), toml::Value::Integer(915_000_000)),
                        ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                        ("spreadingfactor".to_string(), toml::Value::Integer(9)),
                        ("codingrate".to_string(), toml::Value::Integer(5)),
                        ("txpower".to_string(), toml::Value::Integer(-9)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                "radio1".to_string(),
                toml::Value::Table(
                    [
                        ("enabled".to_string(), toml::Value::Boolean(true)),
                        ("vport".to_string(), toml::Value::Integer(3)),
                        ("frequency".to_string(), toml::Value::Integer(920_000_000)),
                        ("bandwidth".to_string(), toml::Value::Integer(125_000)),
                        ("spreadingfactor".to_string(), toml::Value::Integer(10)),
                        ("codingrate".to_string(), toml::Value::Integer(5)),
                        ("txpower".to_string(), toml::Value::Integer(14)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
        ..InterfaceConfig::default()
    };
    let manager = std::sync::Arc::new(tokio::sync::Mutex::new(
        rns_transport::iface::InterfaceManager::new(8),
    ));

    let adapter = rnode_multi::build_adapter(&iface, manager).expect("build rnode multi adapter");

    assert_eq!(adapter.subinterfaces().len(), 1);
    assert_eq!(adapter.subinterfaces()[0].vport, 3);
    assert_eq!(adapter.subinterfaces()[0].config.tx_power_dbm, 14);
}

#[test]
fn ax25_kiss_builder_uses_serial_kiss_base_and_requires_callsign() {
    let iface = InterfaceConfig {
        kind: "ax25_kiss".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyUSB0".to_string()),
        baud_rate: Some(1200),
        callsign: Some("n0call".to_string()),
        ssid: Some(1),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_ax25_adapter(&iface).expect("build ax25 kiss adapter");
    assert_eq!(adapter.device(), "/dev/ttyUSB0");
    assert_eq!(adapter.baud_rate(), 1200);

    let missing = InterfaceConfig {
        kind: "ax25_kiss".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyUSB0".to_string()),
        baud_rate: Some(1200),
        ssid: Some(1),
        ..InterfaceConfig::default()
    };

    let err = match kiss::build_ax25_adapter(&missing) {
        Ok(_) => panic!("missing callsign should fail"),
        Err(err) => err,
    };
    assert!(err.contains("ax25_kiss.callsign"));
}

#[test]
fn kiss_builder_carries_android_beacon_aliases_as_id_beacon() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "android-kiss", port = "/dev/ttyACM0", beacon_interval = 900, beacon_data = "ANDROID-1" }
]
"#,
    )
    .expect("parse Android KISS beacon aliases");
    let iface = &cfg.interfaces[0];

    let adapter = kiss::build_adapter(iface).expect("build kiss adapter");
    let beacon = adapter.kiss_config().id_beacon.expect("id beacon");

    assert_eq!(beacon.callsign, b"ANDROID-1");
    assert_eq!(beacon.interval, std::time::Duration::from_secs(900));
    assert_eq!(beacon.min_payload_len, 15);
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
            i2p_tunneled: Some(true),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select server");
    assert_eq!(selected.bind_addr.as_deref(), Some("0.0.0.0:4242"));
    assert_eq!(selected.selected_index, Some(0));
    assert!(selected.i2p_tunneled);
}

#[test]
fn select_tcp_server_bind_uses_single_backbone_listener_when_transport_not_set() {
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![InterfaceConfig {
            kind: "backbone".to_string(),
            enabled: Some(true),
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            mtu: Some(1_048_576),
            prefer_ipv6: Some(true),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select backbone");
    assert_eq!(selected.bind_addr.as_deref(), Some("127.0.0.1:4242"));
    assert_eq!(selected.selected_index, Some(0));
    assert_eq!(selected.kind, "backbone");
    assert_eq!(selected.client_mtu, Some(1_048_576));
    assert!(selected.prefer_ipv6);
}

#[test]
fn select_tcp_listener_device_ip_honors_prefer_ipv6_and_oper_state() {
    let candidates = [
        ("eth0", std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 10)), true),
        ("eth0", std::net::IpAddr::V6("2001:db8::10".parse().expect("test IPv6")), true),
        ("eth1", std::net::IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 10)), true),
        ("eth0", std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 11)), false),
    ];

    let ipv4 =
        select_tcp_listener_device_ip("eth0", false, &candidates).expect("select IPv4 address");
    let ipv6 =
        select_tcp_listener_device_ip("eth0", true, &candidates).expect("select IPv6 address");

    assert_eq!(ipv4, std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 10)));
    assert_eq!(ipv6, std::net::IpAddr::V6("2001:db8::10".parse().expect("test IPv6")));
}

#[test]
fn select_tcp_server_bind_uses_single_local_listener_when_transport_not_set() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free local port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![InterfaceConfig {
            kind: "local".to_string(),
            enabled: Some(true),
            host: Some("127.0.0.1".to_string()),
            port: Some(port),
            mtu: Some(262_144),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select local");
    let endpoint = format!("127.0.0.1:{port}");
    assert_eq!(selected.bind_addr.as_deref(), Some(endpoint.as_str()));
    assert_eq!(selected.selected_index, Some(0));
    assert_eq!(selected.kind, "local");
    assert_eq!(selected.client_mtu, Some(262_144));
}

#[test]
fn select_tcp_server_bind_ignores_unix_local_listener() {
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![InterfaceConfig {
            kind: "local".to_string(),
            enabled: Some(true),
            shared_instance_type: Some("unix".to_string()),
            socket_path: Some("/tmp/rns-test.sock".to_string()),
            mtu: Some(262_144),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("ignore unix local");
    assert_eq!(selected.bind_addr, None);
    assert_eq!(selected.selected_index, None);
    assert_eq!(selected.kind, "");
    assert_eq!(selected.client_mtu, None);
}

#[test]
fn select_tcp_server_bind_attaches_local_listener_when_port_in_use() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind occupied port");
    let port = listener.local_addr().expect("local addr").port();
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![InterfaceConfig {
            kind: "local".to_string(),
            enabled: Some(true),
            host: Some("127.0.0.1".to_string()),
            port: Some(port),
            mtu: Some(262_144),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select local attach");
    assert_eq!(selected.bind_addr, None);
    assert_eq!(selected.selected_index, None);
    assert_eq!(selected.kind, "local");
    assert_eq!(selected.client_mtu, Some(262_144));
    let endpoint = format!("127.0.0.1:{port}");
    assert_eq!(selected.local_attach_addr.as_deref(), Some(endpoint.as_str()));
    assert_eq!(selected.local_attach_index, Some(0));
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
    assert!(err.contains("multiple enabled TCP listener interfaces"));
}

#[test]
fn select_tcp_server_bind_allows_implicit_shared_local_with_tcp_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free tcp port");
    let tcp_port = listener.local_addr().expect("tcp addr").port();
    drop(listener);
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        display_name: None,
        announce_capabilities: Vec::new(),
        propagation_node: None,
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("127.0.0.1".to_string()),
                port: Some(tcp_port),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "local".to_string(),
                enabled: Some(true),
                synthetic_shared_instance: true,
                shared_instance_type: Some("tcp".to_string()),
                host: Some("127.0.0.1".to_string()),
                port: Some(0),
                ..InterfaceConfig::default()
            },
        ],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select tcp server");

    let endpoint = format!("127.0.0.1:{tcp_port}");
    assert_eq!(selected.bind_addr.as_deref(), Some(endpoint.as_str()));
    assert_eq!(selected.selected_index, Some(0));
    assert_eq!(selected.kind, "tcp_server");
    assert_eq!(selected.local_attach_index, None);
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
