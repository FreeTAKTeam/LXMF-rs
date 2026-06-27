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

#[cfg(feature = "rnode-ble")]
use rns_transport::iface::rnode_ble::{
    native_rnode_identifier_is_excluded, native_rnode_identifier_matches_any,
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

#[cfg(feature = "rnode-ble")]
#[test]
fn rnode_ble_native_identifier_matching_uses_configured_id_and_aliases() {
    let aliases = vec!["RNode Field".to_string(), "AA:BB:CC:DD:EE:FF".to_string()];

    assert!(native_rnode_identifier_matches_any(
        "aa-bb-cc-dd-ee-ff",
        "11:22:33:44:55:66",
        &aliases
    ));
    assert!(native_rnode_identifier_matches_any(
        "112233445566",
        "11:22:33:44:55:66",
        &aliases
    ));
    assert!(!native_rnode_identifier_matches_any(
        "00:00:00:00:00:00",
        "11:22:33:44:55:66",
        &aliases
    ));
}

#[cfg(feature = "rnode-ble")]
#[test]
fn rnode_ble_native_identifier_exclusion_normalizes_android_address() {
    let excluded = vec!["AA:BB:CC:DD:EE:FF".to_string()];

    assert!(native_rnode_identifier_is_excluded("aa-bb-cc-dd-ee-ff", &excluded));
    assert!(!native_rnode_identifier_is_excluded("11:22:33:44:55:66", &excluded));
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
    notifications: std::collections::VecDeque<Option<Vec<u8>>>,
}

impl TestRnodeBleBackend {
    fn with_notifications(notifications: Vec<Vec<u8>>) -> Self {
        Self { notifications: notifications.into_iter().map(Some).collect(), ..Default::default() }
    }

    fn with_notification_sequence(notifications: Vec<Option<Vec<u8>>>) -> Self {
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
        Ok(self.notifications.pop_front().flatten())
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
    #[cfg(feature = "rnode-ble")]
    assert_eq!(
        backend.events,
        vec![
            "connect",
            "subscribe_notifications",
            "next_notification",
            "write",
            "write",
            "write",
            "write",
            "write",
        ]
    );
    #[cfg(not(feature = "rnode-ble"))]
    assert_eq!(
        backend.events,
        vec!["connect", "subscribe_notifications", "write", "write", "write", "write", "write",]
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
    #[cfg(feature = "rnode-ble")]
    let backend = TestRnodeBleBackend::with_notification_sequence(vec![
        None,
        Some(encode_data_frame(&[0xAA, 0xBB])),
    ]);
    #[cfg(not(feature = "rnode-ble"))]
    let backend =
        TestRnodeBleBackend::with_notification_sequence(vec![Some(encode_data_frame(&[
            0xAA, 0xBB,
        ]))]);
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

#[cfg(feature = "rnode-ble")]
#[tokio::test]
async fn rnode_ble_runtime_drains_stale_notifications_before_startup_writes() {
    let backend = TestRnodeBleBackend::with_notification_sequence(vec![
        Some(encode_command_frame(CMD_DETECT, &[DETECT_RESP])),
        Some(encode_data_frame(&[0xAA, 0xBB])),
        None,
    ]);
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    runtime.startup().await.expect("startup");

    let backend = runtime.backend();
    assert_eq!(
        &backend.events[..5],
        &[
            "connect",
            "subscribe_notifications",
            "next_notification",
            "next_notification",
            "next_notification"
        ]
    );
    assert_eq!(backend.writes.len(), 5);
}
