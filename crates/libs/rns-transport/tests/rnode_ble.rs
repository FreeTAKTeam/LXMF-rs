use std::time::Duration;

use rns_transport::iface::kiss::{KissConfig, KissIdBeaconConfig};
use rns_transport::iface::lora::{
    LoraConfig, CMD_BANDWIDTH, CMD_CR, CMD_DETECT, CMD_ERROR, CMD_FREQUENCY, CMD_FW_VERSION,
    CMD_LEAVE, CMD_MCU, CMD_PLATFORM, CMD_RADIO_STATE, CMD_SF, CMD_TXPOWER, DETECT_REQ,
    DETECT_RESP, ERROR_MEMORY_LOW, ERROR_TXFAILED, PLATFORM_ESP32, RADIO_STATE_OFF,
};
use rns_transport::iface::rnode_ble::{
    RnodeBleBackend, RnodeBleCommandMonitor, RnodeBleKissConfig, RnodeBleKissError,
    RnodeBleKissRuntime, RnodeBleKissSession, RnodeBleNotification, RnodeBleWrite,
    RNODE_BLE_CONNECT_TIMEOUT, RNODE_BLE_READ_FRAME_TIMEOUT, RNODE_BLE_SCAN_TIMEOUT,
    RNODE_BLE_SERVICE_UUID, RNODE_BLE_TX_CHARACTERISTIC_UUID, RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
};
use rns_transport::kiss::{
    encode_command_frame, encode_data_frame, CMD_DATA, CMD_P, CMD_READY, CMD_SETHARDWARE,
    CMD_SLOTTIME, CMD_TXDELAY, CMD_TXTAIL, FEND,
};

#[test]
fn rnode_ble_defaults_match_python_nordic_uart_profile() {
    let config = RnodeBleKissConfig::default();

    assert_eq!(RNODE_BLE_SERVICE_UUID, "6E400001-B5A3-F393-E0A9-E50E24DCCA9E");
    assert_eq!(RNODE_BLE_WRITE_CHARACTERISTIC_UUID, "6E400002-B5A3-F393-E0A9-E50E24DCCA9E");
    assert_eq!(RNODE_BLE_TX_CHARACTERISTIC_UUID, "6E400003-B5A3-F393-E0A9-E50E24DCCA9E");
    assert_eq!(RNODE_BLE_SCAN_TIMEOUT, Duration::from_secs(2));
    assert_eq!(RNODE_BLE_CONNECT_TIMEOUT, Duration::from_secs(5));
    assert_eq!(RNODE_BLE_READ_FRAME_TIMEOUT, Duration::from_millis(1_250));
    assert_eq!(config.service_uuid, RNODE_BLE_SERVICE_UUID);
    assert_eq!(config.write_characteristic_uuid, RNODE_BLE_WRITE_CHARACTERISTIC_UUID);
    assert_eq!(config.notify_characteristic_uuid, RNODE_BLE_TX_CHARACTERISTIC_UUID);
    assert_eq!(config.mtu, 508);
    assert_eq!(config.max_write_len, 20);
    assert!(!config.write_with_response);
    assert_eq!(config.kiss.preamble_ms, 350);
    assert_eq!(config.kiss.tx_tail_ms, 20);
    assert_eq!(config.kiss.persistence, 64);
    assert_eq!(config.kiss.slot_time_ms, 20);
    assert!(!config.kiss.flow_control);
}

#[test]
fn rnode_ble_startup_subscribes_before_raw_kiss_configuration() {
    let mut session = RnodeBleKissSession::new(RnodeBleKissConfig::default());

    assert!(!session.status().connected);
    assert!(!session.status().subscribed);

    let writes = session.startup_frames();

    assert!(session.is_subscribed());
    assert!(session.status().subscribed);
    assert_eq!(session.status().pending_writes, 0);
    assert_eq!(
        writes,
        vec![
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_TXDELAY, &[35]),
            },
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_TXTAIL, &[2]),
            },
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_P, &[64]),
            },
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_SLOTTIME, &[2]),
            },
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_READY, &[1]),
            },
        ]
    );
}

#[test]
fn rnode_ble_startup_appends_lora_rnode_initial_frames() {
    let lora_config = LoraConfig::us915_default();
    let mut session = RnodeBleKissSession::new(RnodeBleKissConfig {
        initial_frames: lora_config.command_frames(),
        ..Default::default()
    });

    let writes = session.startup_frames();

    assert_eq!(writes.len(), 5 + lora_config.command_frames().len());
    assert_eq!(
        writes[5],
        RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_command_frame(CMD_DETECT, &[DETECT_REQ]),
        }
    );
    assert_eq!(
        writes.last(),
        Some(&RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_command_frame(CMD_RADIO_STATE, &[1]),
        })
    );
}

#[test]
fn rnode_ble_shutdown_writes_lora_radio_off_and_leave_frames() {
    let lora_config = LoraConfig::us915_default();
    let session = RnodeBleKissSession::new(RnodeBleKissConfig {
        shutdown_frames: lora_config.shutdown_frames(),
        ..Default::default()
    });

    assert_eq!(
        session.shutdown_frames(),
        vec![
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]),
            },
            RnodeBleWrite {
                characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
                with_response: false,
                payload: encode_command_frame(CMD_LEAVE, &[0xff]),
            },
        ]
    );
}

#[test]
fn rnode_ble_session_writes_raw_kiss_frames_without_response() {
    let mut session = RnodeBleKissSession::new(RnodeBleKissConfig::default());
    let payload = [0x01, 0xC0, 0xDB, 0x02];

    let writes = session.enqueue_packet(&payload);

    assert_eq!(
        writes,
        vec![RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_data_frame(&payload),
        }]
    );
}

#[test]
fn rnode_ble_notifications_decode_raw_kiss_payloads() {
    let mut session = RnodeBleKissSession::new(RnodeBleKissConfig::default());

    let packets = session
        .accept_notification(&encode_data_frame(&[0xAA, 0xC0, 0xDB, 0xBB]))
        .expect("decode notification");

    assert_eq!(packets, vec![vec![0xAA, 0xC0, 0xDB, 0xBB]]);
}

#[test]
fn rnode_ble_notifications_preserve_command_responses() {
    let mut session = RnodeBleKissSession::new(RnodeBleKissConfig::default());

    let notification = session
        .accept_notification_events(&encode_command_frame(CMD_SETHARDWARE, &[0x46]))
        .expect("command response notification");

    assert!(notification.packets.is_empty());
    assert_eq!(notification.commands, vec![(CMD_SETHARDWARE, vec![0x46])]);
}

#[test]
fn rnode_ble_flow_control_queues_until_ready_notification() {
    let config = RnodeBleKissConfig {
        kiss: KissConfig { flow_control: true, ..Default::default() },
        ..Default::default()
    };
    let mut session = RnodeBleKissSession::new(config);

    assert!(session.enqueue_packet(&[0x01, 0x02]).is_empty());
    assert_eq!(session.pending_payloads(), 1);

    let packets = session
        .accept_notification(&encode_command_frame(CMD_READY, &[1]))
        .expect("ready notification");

    assert!(packets.is_empty());
    assert_eq!(
        session.take_pending_writes(),
        vec![RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_data_frame(&[0x01, 0x02]),
        }]
    );
}

#[test]
fn rnode_ble_discards_stale_partial_notification_before_next_frame() {
    let config =
        RnodeBleKissConfig { read_frame_timeout: Duration::from_millis(1), ..Default::default() };
    let mut session = RnodeBleKissSession::new(config);

    assert!(session
        .accept_notification(&[FEND, CMD_DATA, b's', b't', b'a', b'l', b'e'])
        .expect("partial notification")
        .is_empty());
    std::thread::sleep(Duration::from_millis(5));

    let packets = session
        .accept_notification(&encode_data_frame(b"fresh"))
        .expect("fresh frame after stale partial");

    assert_eq!(packets, vec![b"fresh".to_vec()]);
}

#[test]
fn rnode_ble_suppresses_own_id_beacon_notification() {
    let config = RnodeBleKissConfig {
        kiss: KissConfig {
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut session = RnodeBleKissSession::new(config);

    let packets = session
        .accept_notification(&encode_data_frame(b"MYCALL-0"))
        .expect("own beacon notification");

    assert!(packets.is_empty());
}

#[test]
fn rnode_ble_session_writes_python_rnode_id_beacon_payload() {
    let config = RnodeBleKissConfig {
        kiss: KissConfig {
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let session = RnodeBleKissSession::new(config);

    assert_eq!(
        session.id_beacon_write(),
        Some(RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_data_frame(b"MYCALL-0"),
        })
    );
}

#[test]
fn rnode_ble_flow_control_queues_id_beacon_until_ready_notification() {
    let config = RnodeBleKissConfig {
        kiss: KissConfig {
            flow_control: true,
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut session = RnodeBleKissSession::new(config);

    assert!(session.enqueue_id_beacon().is_empty());
    assert_eq!(session.pending_payloads(), 1);

    let packets = session
        .accept_notification(&encode_command_frame(CMD_READY, &[1]))
        .expect("ready notification");

    assert!(packets.is_empty());
    assert_eq!(
        session.take_pending_writes(),
        vec![RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_data_frame(b"MYCALL-0"),
        }]
    );
}

#[derive(Default)]
struct TestRnodeBleBackend {
    events: Vec<&'static str>,
    writes: Vec<RnodeBleWrite>,
    notifications: std::collections::VecDeque<Vec<u8>>,
}

impl TestRnodeBleBackend {
    fn with_notifications(notifications: Vec<Vec<u8>>) -> Self {
        Self { notifications: notifications.into(), ..Default::default() }
    }
}

impl RnodeBleBackend for TestRnodeBleBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events.push("connect");
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        self.events.push("subscribe_notifications");
        Ok(())
    }

    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String> {
        self.events.push("write");
        self.writes.push(write);
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.events.push("next_notification");
        Ok(self.notifications.pop_front())
    }
}

#[tokio::test]
async fn rnode_ble_runtime_connects_subscribes_and_writes_startup_frames() {
    let backend = TestRnodeBleBackend::default();
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    assert!(!runtime.status().connected);

    runtime.startup().await.expect("startup");

    assert!(runtime.status().connected);
    assert!(runtime.status().subscribed);
    assert_eq!(runtime.status().pending_payloads, 0);
    assert_eq!(runtime.status().pending_writes, 0);
    let backend = runtime.backend();
    assert_eq!(
        &backend.events[..7],
        &["connect", "subscribe_notifications", "write", "write", "write", "write", "write"]
    );
    assert_eq!(backend.writes.len(), 5);
    assert!(backend
        .writes
        .iter()
        .all(|write| write.characteristic_uuid == RNODE_BLE_WRITE_CHARACTERISTIC_UUID));
    assert!(backend.writes.iter().all(|write| !write.with_response));
}

#[tokio::test]
async fn rnode_ble_runtime_writes_packets_and_polls_notifications() {
    let backend = TestRnodeBleBackend::with_notifications(vec![encode_data_frame(&[0xAA, 0xBB])]);
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    runtime.startup().await.expect("startup");
    runtime.send_packet(&[0x01, 0x02]).await.expect("send packet");
    let packets = runtime.poll_notification().await.expect("poll notification");

    assert_eq!(packets, vec![vec![0xAA, 0xBB]]);
    assert_eq!(
        runtime.backend().writes.last(),
        Some(&RnodeBleWrite {
            characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            with_response: false,
            payload: encode_data_frame(&[0x01, 0x02]),
        })
    );
}

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
