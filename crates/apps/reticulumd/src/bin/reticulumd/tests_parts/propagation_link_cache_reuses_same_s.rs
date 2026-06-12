#[tokio::test]
async fn propagation_link_cache_reuses_same_selected_node() {
    let (_daemon, bridge) = test_transport_bridge_fixture().await;
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let destination = DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: *identity.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };

    let first = bridge.propagation_link_for_test("peer-a", destination).await;
    let second = bridge.propagation_link_for_test("peer-a", destination).await;

    let first_id = *first.lock().await.id();
    let second_id = *second.lock().await.id();
    assert_eq!(first_id, second_id);
}

#[tokio::test]
async fn propagation_link_cache_does_not_close_previous_link_when_selected_node_changes() {
    let (_daemon, bridge) = test_transport_bridge_fixture().await;
    let signer_a = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity_a = rns_transport::identity_bridge::to_transport_private_identity(&signer_a);
    let destination_a = DestinationDesc {
        identity: *identity_a.as_identity(),
        address_hash: *identity_a.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };
    let signer_b = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity_b = rns_transport::identity_bridge::to_transport_private_identity(&signer_b);
    let destination_b = DestinationDesc {
        identity: *identity_b.as_identity(),
        address_hash: *identity_b.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };

    let first = bridge.propagation_link_for_test("peer-a", destination_a).await;
    let second = bridge.propagation_link_for_test("peer-b", destination_b).await;

    let first_id = *first.lock().await.id();
    let second_id = *second.lock().await.id();
    assert_ne!(first_id, second_id);
    assert_ne!(first.lock().await.status(), LinkStatus::Closed);
}

#[tokio::test]
async fn propagation_link_cache_recreates_closed_link_for_same_selected_node() {
    let (_daemon, bridge) = test_transport_bridge_fixture().await;
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let destination = DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: *identity.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };

    let first = bridge.propagation_link_for_test("peer-a", destination).await;
    let first_id = *first.lock().await.id();
    first.lock().await.close();

    let second = bridge.propagation_link_for_test("peer-a", destination).await;

    assert_ne!(first_id, *second.lock().await.id());
}

#[test]
fn parse_destination_hex_required_rejects_invalid_hashes() {
    let err = parse_destination_hash_required("not-hex").expect_err("invalid hash");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn tcp_client_adapter_exposes_default_mtu() {
    let adapter = TcpClient::new("rmap.world:4242");
    assert_eq!(adapter.mtu_value(), TcpClient::DEFAULT_MTU);
}

#[test]
fn serial_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = serial::build_adapter(&iface);
    assert!(result.is_err(), "missing device/baud should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("serial.device"));
}

#[test]
fn serial_builder_rejects_zero_baud_rate() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyUSB0".to_string()),
        baud_rate: Some(0),
        ..InterfaceConfig::default()
    };
    let err = serial::build_adapter(&iface).err().expect("zero baud rate should fail");
    assert!(err.contains("serial.baud_rate must be > 0"));
}

#[test]
fn serial_builder_accepts_python_serial_line_alias_values() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyUSB0".to_string()),
        baud_rate: Some(19_200),
        data_bits: Some(7),
        parity: Some("N".to_string()),
        stop_bits: Some(2),
        ..InterfaceConfig::default()
    };

    let adapter = serial::build_adapter(&iface).expect("build serial adapter");
    assert_eq!(adapter.device(), "/dev/ttyUSB0");
    assert_eq!(adapter.baud_rate(), 19_200);
    assert_eq!(adapter.data_bits_value(), 7);
    assert_eq!(adapter.parity_name(), "none");
    assert_eq!(adapter.stop_bits_value(), 2);
}

#[test]
fn kiss_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "kiss".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = kiss::build_adapter(&iface);
    assert!(result.is_err(), "missing device/baud should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("kiss.device"));
}

#[test]
fn kiss_builder_uses_serial_line_settings() {
    let iface = InterfaceConfig {
        kind: "kiss".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyACM0".to_string()),
        baud_rate: Some(19_200),
        data_bits: Some(7),
        parity: Some("E".to_string()),
        stop_bits: Some(2),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_adapter(&iface).expect("build kiss adapter");
    assert_eq!(adapter.device(), "/dev/ttyACM0");
    assert_eq!(adapter.baud_rate(), 19_200);
    assert_eq!(adapter.data_bits_value(), 7);
    assert_eq!(adapter.parity_name(), "even");
    assert_eq!(adapter.stop_bits_value(), 2);
}

#[test]
fn kiss_tcp_client_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = kiss::build_tcp_client_adapter(&iface);
    assert!(result.is_err(), "missing host/port should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("kiss_tcp_client.host"));
}

#[test]
fn kiss_tcp_client_builder_uses_endpoint_and_kiss_overrides() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        host: Some("192.0.2.10".to_string()),
        port: Some(8001),
        mtu: Some(512),
        preamble_ms: Some(410),
        tx_tail_ms: Some(30),
        persistence: Some(80),
        slot_time_ms: Some(40),
        kiss_flow_control: Some(true),
        id_callsign: Some("MYCALL-0".to_string()),
        id_interval: Some(600),
        reconnect_backoff_ms: Some(100),
        max_reconnect_backoff_ms: Some(200),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_tcp_client_adapter(&iface).expect("build kiss tcp client adapter");
    assert_eq!(adapter.addr(), "192.0.2.10:8001");
    assert_eq!(adapter.mtu(), 512);
    assert_eq!(adapter.reconnect_backoff(), Duration::from_millis(100));
    assert_eq!(adapter.max_reconnect_backoff(), Duration::from_millis(200));
    assert_eq!(
        adapter.kiss_config(),
        rns_transport::iface::kiss::KissConfig {
            preamble_ms: 410,
            tx_tail_ms: 30,
            persistence: 80,
            slot_time_ms: 40,
            flow_control: true,
            id_beacon: Some(rns_transport::iface::kiss::KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 15,
            }),
        }
    );
}

#[test]
fn kiss_tcp_client_builder_preserves_python_empty_id_beacon_when_callsign_missing() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        host: Some("192.0.2.10".to_string()),
        port: Some(8001),
        id_interval: Some(600),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_tcp_client_adapter(&iface).expect("build kiss tcp client adapter");

    assert_eq!(
        adapter.kiss_config().id_beacon,
        Some(rns_transport::iface::kiss::KissIdBeaconConfig {
            callsign: Vec::new(),
            interval: Duration::from_secs(600),
            min_payload_len: 15,
        })
    );
}

#[test]
fn kiss_tcp_client_builder_supports_tcp_client_kiss_framing_alias_output() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        host: Some("192.0.2.10".to_string()),
        port: Some(8001),
        mtu: Some(512),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_tcp_client_adapter(&iface).expect("build kiss tcp client adapter");
    assert_eq!(adapter.addr(), "192.0.2.10:8001");
    assert_eq!(adapter.mtu(), 512);
}

#[test]
fn lora_builder_uses_region_defaults_and_config_overrides() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        device: Some("/dev/ttyACM1".to_string()),
        baud_rate: Some(115200),
        bandwidth_hz: Some(250_000),
        spreading_factor: Some(8),
        coding_rate: Some("4/6".to_string()),
        tx_power_dbm: Some(14),
        airtime_limit_short: Some(33.0),
        airtime_limit_long: Some(1.5),
        max_payload_bytes: Some(180),
        flow_control: Some(toml::Value::Boolean(true)),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build lora adapter");
    assert_eq!(adapter.config().frequency_hz, 915_000_000);
    assert_eq!(adapter.config().bandwidth_hz, 250_000);
    assert_eq!(adapter.config().spreading_factor, 8);
    assert_eq!(adapter.config().coding_rate, 6);
    assert_eq!(adapter.config().tx_power_dbm, 14);
    assert_eq!(adapter.config().airtime_limit_short_hundredths, Some(3_300));
    assert_eq!(adapter.config().airtime_limit_long_hundredths, Some(150));
    assert_eq!(adapter.config().max_payload_bytes, 180);
    assert!(adapter.flow_control());
}

#[test]
fn lora_builder_supports_python_rnode_tcp_port() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("tcp://192.0.2.10:8001".to_string()),
        frequency_hz: Some(915_000_000),
        bandwidth_hz: Some(125_000),
        spreading_factor: Some(9),
        coding_rate: Some("5".to_string()),
        tx_power_dbm: Some(17),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build tcp rnode adapter");

    assert_eq!(adapter.bearer(), rns_transport::iface::lora::LoraBearer::Tcp);
    assert_eq!(adapter.endpoint(), "192.0.2.10:8001");
    assert_eq!(adapter.baud_rate(), None);
}

#[test]
fn lora_builder_supports_python_high_bandwidth_rnode_config() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("tcp://192.0.2.10:8001".to_string()),
        frequency_hz: Some(2_400_000_000),
        bandwidth_hz: Some(1_625_000),
        spreading_factor: Some(5),
        coding_rate: Some("5".to_string()),
        tx_power_dbm: Some(17),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build high-bandwidth rnode adapter");

    assert_eq!(adapter.config().frequency_hz, 2_400_000_000);
    assert_eq!(adapter.config().bandwidth_hz, 1_625_000);
    assert_eq!(adapter.config().spreading_factor, 5);
}

#[test]
fn lora_builder_uses_python_rnode_command_timeout() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("/dev/ttyACM1".to_string()),
        baud_rate: Some(115200),
        connect_timeout_ms: Some(2_750),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build lora adapter");

    assert_eq!(adapter.startup_response_timeout(), Duration::from_millis(2_750));
}

#[test]
fn rnode_ble_builder_uses_native_ble_and_kiss_defaults() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("rnode-ble".to_string()),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("ble://RNode 1234".to_string()),
        adapter: Some("Bluetooth".to_string()),
        mtu: Some(512),
        max_write_len: Some(64),
        max_payload_bytes: Some(220),
        scan_timeout_ms: Some(3_000),
        ble_connect_timeout_ms: Some(7_000),
        connect_timeout_ms: Some(4_000),
        preamble_ms: Some(410),
        tx_tail_ms: Some(30),
        persistence: Some(80),
        slot_time_ms: Some(40),
        flow_control: Some(toml::Value::Boolean(true)),
        ..InterfaceConfig::default()
    };

    let config = lora::build_rnode_ble_config(&iface).expect("build rnode BLE config");

    assert_eq!(config.peripheral_id, "RNode 1234");
    assert_eq!(config.adapter.as_deref(), Some("Bluetooth"));
    assert_eq!(config.transport.mtu, 220);
    assert_eq!(config.transport.max_write_len, 64);
    assert_eq!(config.transport.scan_timeout, Duration::from_millis(3_000));
    assert_eq!(config.transport.connect_timeout, Duration::from_millis(7_000));
    assert_eq!(config.startup_response_timeout, Duration::from_millis(4_000));
    assert_eq!(config.transport.kiss.preamble_ms, 410);
    assert_eq!(config.transport.kiss.tx_tail_ms, 30);
    assert_eq!(config.transport.kiss.persistence, 80);
    assert_eq!(config.transport.kiss.slot_time_ms, 40);
    assert!(config.transport.kiss.flow_control);
    // initial_frames carries only probe frames (Phase 1: detect handshake)
    assert_eq!(
        config.transport.initial_frames.first(),
        Some(&rns_transport::kiss::encode_command_frame(CMD_DETECT, &[DETECT_REQ]))
    );
    assert_eq!(
        config.transport.initial_frames.last(),
        Some(&rns_transport::kiss::encode_command_frame(CMD_MCU, &[0x00]))
    );
    // deferred_frames carries radio config (Phase 2: sent after detect confirmed)
    assert_eq!(
        config.transport.deferred_frames.first(),
        Some(&rns_transport::kiss::encode_command_frame(
            CMD_FREQUENCY,
            &915_000_000_u32.to_be_bytes()
        ))
    );
    assert_eq!(
        config.transport.deferred_frames.last(),
        Some(&rns_transport::kiss::encode_command_frame(CMD_RADIO_STATE, &[1]))
    );
    assert_eq!(
        config.transport.shutdown_frames,
        vec![
            rns_transport::kiss::encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]),
            rns_transport::kiss::encode_command_frame(CMD_LEAVE, &[0xff]),
        ]
    );
}

#[test]
fn rnode_ble_builder_keeps_ble_connect_timeout_distinct_from_rnode_command_timeout() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("rnode-ble".to_string()),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("ble://RNode 1234".to_string()),
        frequency_hz: Some(915_000_000),
        bandwidth_hz: Some(125_000),
        spreading_factor: Some(9),
        coding_rate: Some("5".to_string()),
        tx_power_dbm: Some(17),
        ..InterfaceConfig::default()
    };

    let config = lora::build_rnode_ble_config(&iface).expect("build rnode BLE config");

    // BLE physical connect timeout and RNode detect timeout are separate fields,
    // configured independently via ble_connect_timeout_ms and connect_timeout_ms.
    assert_eq!(config.transport.connect_timeout, Duration::from_millis(5_000));
    assert_eq!(config.startup_response_timeout, Duration::from_millis(5_000)); // matches Python's ble_detect_timeout
}
