#[cfg(feature = "vrn76-kiss-ble")]
#[test]
fn native_vrn76_ble_settings_use_profile_defaults() {
    use rns_transport::iface::vrn76_kiss_ble::{
        NativeVrn76BleSettings, VRN76_INDICATE_CHARACTERISTIC_UUID, VRN76_SERVICE_UUID,
        VRN76_WRITE_CHARACTERISTIC_UUID,
    };

    let settings = NativeVrn76BleSettings::for_peripheral("VR-N76");

    assert_eq!(settings.peripheral_id, "VR-N76");
    assert_eq!(settings.service_uuid.to_string(), VRN76_SERVICE_UUID);
    assert_eq!(settings.write_uuid.to_string(), VRN76_WRITE_CHARACTERISTIC_UUID);
    assert_eq!(settings.indicate_uuid.to_string(), VRN76_INDICATE_CHARACTERISTIC_UUID);
    assert_eq!(settings.scan_timeout, Duration::from_millis(10_000));
    assert_eq!(settings.connect_timeout, Duration::from_millis(3_000));
    assert_eq!(settings.notification_timeout, Duration::from_millis(3_000));
}

#[cfg(feature = "vrn76-kiss-ble")]
#[test]
fn native_vrn76_identifier_matching_normalizes_addresses_and_names() {
    use rns_transport::iface::vrn76_kiss_ble::native_vrn76_identifier_matches;

    assert!(native_vrn76_identifier_matches("AA:BB:CC:DD", "aabbccdd"));
    assert!(native_vrn76_identifier_matches("vr-n76", "VR-N76"));
    assert!(native_vrn76_identifier_matches("AB-CD-EF", "abcdef"));
    assert!(!native_vrn76_identifier_matches("AB-CD-EF", "abcdee"));
}

#[derive(Default)]
struct FakeBleBackend {
    events: Vec<&'static str>,
    writes: Vec<BleWrite>,
    indications: VecDeque<Vec<u8>>,
    indication_errors: VecDeque<String>,
    write_error_on_call: Option<usize>,
    write_error: String,
    reject_startup_command_writes: bool,
}

impl FakeBleBackend {
    fn with_indications(indications: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self { indications: indications.into_iter().collect(), ..Default::default() }
    }

    fn with_indication_errors(errors: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            indication_errors: errors.into_iter().map(str::to_string).collect(),
            ..Default::default()
        }
    }

    fn with_write_error_on_call(call_index: usize, error: &'static str) -> Self {
        Self {
            write_error_on_call: Some(call_index),
            write_error: error.to_string(),
            ..Default::default()
        }
    }

    fn rejecting_startup_command_writes() -> Self {
        Self { reject_startup_command_writes: true, ..Default::default() }
    }
}

impl Vrn76KissBleBackend for FakeBleBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events.push("connect");
        Ok(())
    }

    async fn subscribe_indications(&mut self) -> Result<(), String> {
        self.events.push("subscribe");
        Ok(())
    }

    async fn write(&mut self, write: BleWrite) -> Result<(), String> {
        self.events.push("write");
        if self.reject_startup_command_writes && is_startup_kiss_command_write(&write) {
            return Err("unsupported KISS command".to_string());
        }
        if self.write_error_on_call == Some(self.writes.len()) {
            return Err(self.write_error.clone());
        }
        self.writes.push(write);
        Ok(())
    }

    async fn next_indication(&mut self) -> Result<Option<Vec<u8>>, String> {
        if let Some(error) = self.indication_errors.pop_front() {
            return Err(error);
        }
        Ok(self.indications.pop_front())
    }
}

fn is_startup_kiss_command_write(write: &BleWrite) -> bool {
    let commands = [CMD_TXDELAY, CMD_TXTAIL, CMD_P, CMD_SLOTTIME, CMD_FULLDUPLEX, CMD_READY];
    commands.iter().any(|command| {
        write.payload == encode_benshi_ht_send_data(&encode_command_frame(*command, &[0]))
            || write.payload == encode_benshi_ht_send_data(&encode_command_frame(*command, &[1]))
            || write.payload == encode_benshi_ht_send_data(&encode_command_frame(*command, &[2]))
            || write.payload == encode_benshi_ht_send_data(&encode_command_frame(*command, &[20]))
            || write.payload == encode_benshi_ht_send_data(&encode_command_frame(*command, &[35]))
            || write.payload == encode_benshi_ht_send_data(&encode_command_frame(*command, &[64]))
    })
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_connects_subscribes_then_writes_startup_commands() {
    let mut runtime =
        Vrn76KissBleRuntime::new(FakeBleBackend::default(), Vrn76KissBleConfig::default());

    assert!(!runtime.status().connected);

    runtime.connect_and_configure().await.expect("connect and configure");
    let status = runtime.status();
    assert!(status.connected);
    assert!(status.subscribed);
    assert!(status.interface_ready);
    let backend = runtime.into_backend();

    assert_eq!(&backend.events[..2], ["connect", "subscribe"]);
    assert!(backend.events[2..].iter().all(|event| *event == "write"));
    assert!(backend
        .writes
        .iter()
        .any(|write| write.payload
            == encode_benshi_ht_send_data(&encode_command_frame(CMD_P, &[64]))));
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_continues_when_startup_kiss_commands_are_rejected() {
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::rejecting_startup_command_writes(),
        Vrn76KissBleConfig::default(),
    );

    runtime.connect_and_configure().await.expect("startup command rejection should not fail link");
    assert!(runtime.status().connected);
    assert!(runtime.status().startup_write_failures > 0);

    runtime.send_packet(&[0xAA, 0xBB]).await.expect("data write should still work");
    let backend = runtime.into_backend();

    assert_eq!(
        backend.writes,
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&[0xAA, 0xBB])),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_sends_outbound_packets_through_backend() {
    let mut runtime =
        Vrn76KissBleRuntime::new(FakeBleBackend::default(), Vrn76KissBleConfig::default());

    runtime.send_packet(&[0xAA, 0xBB]).await.expect("send packet");
    let backend = runtime.into_backend();

    assert_eq!(
        backend.writes,
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&[0xAA, 0xBB])),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_writes_python_kiss_id_beacon_through_backend() {
    let config = Vrn76KissBleConfig {
        kiss: KissConfig {
            id_beacon: Some(KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 15,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut runtime = Vrn76KissBleRuntime::new(FakeBleBackend::default(), config);
    let mut expected_payload = b"MYCALL-0".to_vec();
    expected_payload.resize(15, 0);

    runtime.send_id_beacon().await.expect("send id beacon");

    assert_eq!(
        runtime.into_backend().writes,
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&expected_payload)),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_rejects_outbound_packets_larger_than_mtu_before_ble_write() {
    let config = Vrn76KissBleConfig { mtu: 2, ..Default::default() };
    let mut runtime = Vrn76KissBleRuntime::new(FakeBleBackend::default(), config);

    let err = runtime.send_packet(&[0xAA, 0xBB, 0xCC]).await.expect_err("oversized packet");

    assert_eq!(err, Vrn76KissBleError::PacketTooLarge { limit: 2, actual: 3 });
    assert!(runtime.into_backend().writes.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_polls_indications_into_packet_payloads() {
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::with_indications([encode_benshi_data_rxd_event(&encode_data_frame(&[
            0x01, 0x02,
        ]))]),
        Vrn76KissBleConfig::default(),
    );

    let packet = runtime.poll_next_packet().await.expect("poll").expect("packet");

    assert_eq!(packet, vec![0x01, 0x02]);
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_preserves_multiple_packets_from_one_indication() {
    let mut kiss_stream = encode_data_frame(&[0x01, 0x02]);
    kiss_stream.extend_from_slice(&encode_data_frame(&[0x03, 0x04]));
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::with_indications([encode_benshi_data_rxd_event(&kiss_stream)]),
        Vrn76KissBleConfig::default(),
    );

    let first = runtime.poll_next_packet().await.expect("first poll").expect("first packet");
    assert_eq!(runtime.status().pending_packets, 1);
    let second = runtime.poll_next_packet().await.expect("second poll").expect("second packet");
    assert_eq!(runtime.status().pending_packets, 0);
    let empty = runtime.poll_next_packet().await.expect("third poll");

    assert_eq!(first, vec![0x01, 0x02]);
    assert_eq!(second, vec![0x03, 0x04]);
    assert!(empty.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_reconnect_clears_stale_pending_packets() {
    let mut kiss_stream = encode_data_frame(&[0x01, 0x02]);
    kiss_stream.extend_from_slice(&encode_data_frame(&[0x03, 0x04]));
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::with_indications([encode_benshi_data_rxd_event(&kiss_stream)]),
        Vrn76KissBleConfig::default(),
    );

    let first = runtime.poll_next_packet().await.expect("first poll").expect("first packet");
    assert_eq!(first, vec![0x01, 0x02]);
    assert_eq!(runtime.status().pending_packets, 1);

    runtime.connect_and_configure().await.expect("reconnect and configure");

    assert_eq!(runtime.status().pending_packets, 0);
    assert!(runtime.poll_next_packet().await.expect("poll after reconnect").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_flushes_pending_flow_control_writes_after_ready() {
    let config = Vrn76KissBleConfig {
        kiss: rns_transport::iface::kiss::KissConfig { flow_control: true, ..Default::default() },
        ..Default::default()
    };
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::with_indications([encode_benshi_data_rxd_event(&encode_command_frame(
            CMD_READY,
            &[0x01],
        ))]),
        config,
    );

    runtime.send_packet(&[0x10, 0x20]).await.expect("queue packet");
    assert!(runtime.backend().writes.is_empty());

    assert!(runtime.poll_next_packet().await.expect("ready").is_none());

    assert_eq!(
        runtime.into_backend().writes,
        vec![BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload: encode_benshi_ht_send_data(&encode_data_frame(&[0x10, 0x20])),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_marks_disconnected_when_indication_backend_fails() {
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::with_indication_errors(["link lost"]),
        Vrn76KissBleConfig::default(),
    );
    runtime.connect_and_configure().await.expect("connect and configure");
    assert!(runtime.status().connected);

    let err = runtime.poll_next_packet().await.expect_err("backend indication failure");

    assert_eq!(
        err,
        Vrn76KissBleError::Backend {
            operation: "next_indication",
            message: "link lost".to_string(),
        }
    );
    assert!(!runtime.status().connected);
}

#[tokio::test(flavor = "current_thread")]
async fn vrn76_runtime_marks_disconnected_when_ble_write_fails_after_connect() {
    let startup_write_count =
        Vrn76KissBleSession::new(Vrn76KissBleConfig::default()).startup_frames().len();
    let mut runtime = Vrn76KissBleRuntime::new(
        FakeBleBackend::with_write_error_on_call(startup_write_count, "write failed"),
        Vrn76KissBleConfig::default(),
    );
    runtime.connect_and_configure().await.expect("connect and configure");
    assert!(runtime.status().connected);

    let err = runtime.send_packet(&[0x01, 0x02]).await.expect_err("backend write failure");

    assert_eq!(
        err,
        Vrn76KissBleError::Backend {
            operation: "write_packet",
            message: "write failed".to_string(),
        }
    );
    assert!(!runtime.status().connected);
}
