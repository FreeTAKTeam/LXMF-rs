#[tokio::test]
async fn rnode_ble_runtime_polls_command_notification_events() {
    let backend = TestRnodeBleBackend::with_notifications(vec![encode_command_frame(
        CMD_SETHARDWARE,
        &[0x46],
    )]);
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    let notification = runtime.poll_notification_events().await.expect("poll notification");

    assert!(notification.packets.is_empty());
    assert_eq!(notification.commands, vec![(CMD_SETHARDWARE, vec![0x46])]);
}

#[tokio::test]
async fn rnode_ble_runtime_rejects_outbound_packets_larger_than_mtu_before_ble_write() {
    let config = RnodeBleKissConfig { mtu: 4, ..Default::default() };
    let backend = TestRnodeBleBackend::default();
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.startup().await.expect("startup");
    let startup_writes = runtime.backend().writes.len();
    let err = runtime.send_packet(&[0, 1, 2, 3, 4]).await.expect_err("payload exceeds mtu");

    assert_eq!(err, RnodeBleKissError::PacketTooLarge { limit: 4, actual: 5 });
    assert_eq!(
        runtime.backend().writes.len(),
        startup_writes,
        "oversized packet must fail before any BLE write"
    );
}

#[tokio::test]
async fn rnode_ble_runtime_splits_outbound_kiss_frames_by_ble_write_limit() {
    let config = RnodeBleKissConfig { max_write_len: 4, ..Default::default() };
    let backend = TestRnodeBleBackend::default();
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.startup().await.expect("startup");
    runtime.send_packet(&[1, 2, 3, 4, 5]).await.expect("send packet");

    let packet_writes = &runtime.backend().writes[5..];
    assert_eq!(
        packet_writes,
        &[
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: vec![0xC0, 0x00, 1, 2],
            },
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: vec![3, 4, 5, 0xC0],
            },
        ]
    );
}

#[tokio::test]
async fn rnode_ble_runtime_writes_configured_shutdown_frames() {
    let config = RnodeBleKissConfig {
        shutdown_frames: vec![encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF])],
        ..Default::default()
    };
    let backend = TestRnodeBleBackend::default();
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.startup().await.expect("startup");
    runtime.shutdown().await.expect("shutdown");

    assert_eq!(
        runtime.backend().writes.last(),
        Some(&RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]),
        })
    );
}

#[test]
fn rnode_ble_command_monitor_accepts_valid_startup_responses() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);

    monitor
        .accept_notification(&RnodeBleNotification {
            packets: Vec::new(),
            commands: valid_startup_commands(config),
        })
        .expect("valid command responses");

    monitor.validate_startup_deadline().expect("startup responses validate");
}

#[test]
fn rnode_ble_command_monitor_exposes_rnode_protocol_state() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);
    let mut commands = valid_startup_commands(config);
    commands.push((CMD_ERROR, vec![ERROR_MEMORY_LOW]));

    monitor
        .accept_notification(&RnodeBleNotification { packets: vec![vec![0x01, 0x02]], commands })
        .expect("valid command responses");

    assert_eq!(monitor.probe_status().platform, Some(PLATFORM_ESP32));
    assert_eq!(monitor.radio_status().bandwidth_hz, Some(config.bandwidth_hz));
    assert!(monitor.online());
    assert_eq!(monitor.last_command_error(), None);
    assert_eq!(monitor.hardware_errors().len(), 1);
    assert!(!monitor.hardware_errors()[0].fatal);
    assert!(monitor.reported_bitrate_bps().is_some());
    assert_eq!(monitor.radio_status().rssi_dbm, None);
    assert_eq!(monitor.radio_status().snr_db, None);
}

#[test]
fn rnode_ble_command_monitor_rejects_missing_startup_responses_after_deadline() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);

    let err = monitor.validate_startup_deadline().expect_err("missing startup responses");

    assert!(err.contains("detect"), "unexpected startup error: {err}");
}

#[test]
fn rnode_ble_command_monitor_rejects_fatal_hardware_errors() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::from_secs(1));

    let err = monitor
        .accept_notification(&RnodeBleNotification {
            packets: Vec::new(),
            commands: vec![(CMD_ERROR, vec![ERROR_TXFAILED])],
        })
        .expect_err("fatal hardware error");

    assert_eq!(err, "Hardware transmit failure");
}

#[cfg(feature = "rnode-ble")]
#[test]
fn native_rnode_ble_settings_use_profile_defaults() {
    use rns_transport::iface::rnode_ble::{
        NativeRnodeBleSettings, RNODE_BLE_CONNECT_TIMEOUT, RNODE_BLE_READ_FRAME_TIMEOUT,
        RNODE_BLE_SCAN_TIMEOUT,
    };

    let settings = NativeRnodeBleSettings::for_peripheral("RNode 1234");

    assert_eq!(settings.peripheral_id, "RNode 1234");
    assert_eq!(settings.service_uuid.to_string(), RNODE_BLE_SERVICE_UUID.to_ascii_lowercase());
    assert_eq!(
        settings.write_uuid.to_string(),
        RNODE_BLE_WRITE_CHARACTERISTIC_UUID.to_ascii_lowercase()
    );
    assert_eq!(
        settings.notify_uuid.to_string(),
        RNODE_BLE_TX_CHARACTERISTIC_UUID.to_ascii_lowercase()
    );
    assert_eq!(settings.scan_timeout, RNODE_BLE_SCAN_TIMEOUT);
    assert_eq!(settings.connect_timeout, RNODE_BLE_CONNECT_TIMEOUT);
    assert_eq!(settings.notification_timeout, RNODE_BLE_READ_FRAME_TIMEOUT);
}

fn valid_startup_commands(config: LoraConfig) -> Vec<(u8, Vec<u8>)> {
    vec![
        (CMD_DETECT, vec![DETECT_RESP]),
        (CMD_FW_VERSION, vec![1, 52]),
        (CMD_PLATFORM, vec![PLATFORM_ESP32]),
        (CMD_MCU, vec![0x01]),
        (
            CMD_FREQUENCY,
            u32::try_from(config.frequency_hz)
                .expect("validated LoRa frequency fits u32")
                .to_be_bytes()
                .to_vec(),
        ),
        (CMD_BANDWIDTH, config.bandwidth_hz.to_be_bytes().to_vec()),
        (CMD_TXPOWER, vec![config.tx_power_dbm as u8]),
        (CMD_SF, vec![config.spreading_factor]),
        (CMD_CR, vec![config.coding_rate]),
        (CMD_RADIO_STATE, vec![1]),
    ]
}

#[cfg(feature = "rnode-ble")]
#[test]
fn native_rnode_identifier_matching_normalizes_addresses_and_names() {
    use rns_transport::iface::rnode_ble::native_rnode_identifier_matches;

    assert!(native_rnode_identifier_matches("AA:BB:CC:DD", "aabbccdd"));
    assert!(native_rnode_identifier_matches("RNode-1234", "rnode1234"));
    assert!(native_rnode_identifier_matches("AB-CD-EF", "abcdef"));
    assert!(!native_rnode_identifier_matches("AB-CD-EF", "abcdee"));
}
