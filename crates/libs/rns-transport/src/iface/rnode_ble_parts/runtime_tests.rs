use crate::iface::lora::{
    CMD_BANDWIDTH, CMD_BLINK, CMD_CR, CMD_DETECT, CMD_FB_EXT, CMD_FREQUENCY, CMD_FW_VERSION,
    CMD_LEAVE, CMD_MCU, CMD_PLATFORM, CMD_RADIO_STATE, CMD_SF, CMD_TXPOWER, DETECT_RESP,
    PLATFORM_ESP32, RADIO_STATE_ASK, RADIO_STATE_OFF, RADIO_STATE_ON,
};
use crate::kiss::{decode_frames, encode_command_frame};

#[derive(Default)]
struct TestBackend {
    negotiated_mtu: Option<u16>,
    notifications: VecDeque<Vec<u8>>,
    writes: Vec<RnodeBleWrite>,
}

impl RnodeBleBackend for TestBackend {
    async fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String> {
        self.writes.push(write);
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.notifications.pop_front())
    }

    fn negotiated_mtu(&self) -> Option<u16> {
        self.negotiated_mtu
    }
}

#[test]
fn rnode_queue_admission_sends_one_packet_per_ready_response() {
    let config = RnodeBleKissConfig {
        max_write_len: 508,
        kiss: KissConfig { flow_control: true, ..KissConfig::default() },
        ..RnodeBleKissConfig::default()
    };
    let mut session = RnodeBleKissSession::new(config);
    let _startup = session.startup_frames();

    assert!(session.enqueue_packet(b"first").is_empty());
    assert!(session.enqueue_packet(b"second").is_empty());
    assert_eq!(session.status().pending_payloads, 2);

    session
        .accept_notification_events(&encode_command_frame(CMD_READY, &[1]))
        .expect("accept ready response");
    let first_writes = session.take_pending_writes();
    let first_frames = decode_frames(
        &first_writes.into_iter().flat_map(|write| write.payload).collect::<Vec<_>>(),
        508,
    )
    .expect("decode first admitted write");
    assert_eq!(
        first_frames,
        vec![
            KissFrame::Data(b"first".to_vec()),
            KissFrame::Command(KissCommand::Ready),
        ]
    );
    assert!(!session.status().interface_ready);
    assert_eq!(session.status().pending_payloads, 1);

    session
        .accept_notification_events(&encode_command_frame(CMD_READY, &[0]))
        .expect("accept full-queue response");
    assert!(session.take_pending_writes().is_empty());
    assert!(!session.status().interface_ready);

    session.last_queue_admission_probe_at =
        Some(Instant::now() - RNODE_QUEUE_ADMISSION_POLL_INTERVAL);
    let retry_probe = session.queue_admission_probe_if_due();
    assert_eq!(
        decode_frames(
            &retry_probe.into_iter().flat_map(|write| write.payload).collect::<Vec<_>>(),
            508,
        )
        .expect("decode retry probe"),
        vec![KissFrame::Command(KissCommand::Ready)]
    );
    assert!(session.queue_admission_probe_if_due().is_empty());

    session
        .accept_notification_events(&encode_command_frame(CMD_READY, &[1]))
        .expect("accept drained-queue response");
    let second_writes = session.take_pending_writes();
    let second_frames = decode_frames(
        &second_writes.into_iter().flat_map(|write| write.payload).collect::<Vec<_>>(),
        508,
    )
    .expect("decode second admitted write");
    assert_eq!(
        second_frames,
        vec![
            KissFrame::Data(b"second".to_vec()),
            KissFrame::Command(KissCommand::Ready),
        ]
    );
    assert_eq!(session.status().pending_payloads, 0);
}

fn startup_notification_without_radio_state(config: LoraConfig) -> RnodeBleNotification {
    RnodeBleNotification {
        commands: vec![
            (CMD_DETECT, vec![DETECT_RESP]),
            (CMD_FW_VERSION, vec![1, 83]),
            (CMD_PLATFORM, vec![PLATFORM_ESP32]),
            (CMD_MCU, vec![1]),
            (
                CMD_FREQUENCY,
                u32::try_from(config.frequency_hz)
                    .expect("test frequency fits RNode response")
                    .to_be_bytes()
                    .to_vec(),
            ),
            (CMD_BANDWIDTH, config.bandwidth_hz.to_be_bytes().to_vec()),
            (CMD_TXPOWER, config.tx_power_dbm.to_be_bytes().to_vec()),
            (CMD_SF, vec![config.spreading_factor]),
            (CMD_CR, vec![config.coding_rate]),
        ],
        packets: Vec::new(),
    }
}

#[test]
fn startup_accepts_only_missing_radio_state_for_compatible_firmware() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);
    monitor
        .accept_notification(&startup_notification_without_radio_state(config))
        .expect("accept startup responses");

    monitor
        .validate_startup_deadline()
        .expect("missing radio-state echo should use compatibility mode");

    assert!(monitor.online());
    let status = monitor.runtime_status_json("ble://legacy-rnode");
    assert_eq!(status["last_command_error"], serde_json::Value::Null);
    assert!(status["startup_compatibility_warning"]
        .as_str()
        .is_some_and(|warning| warning.contains("omitted the startup radio-state response")));
}

#[test]
fn startup_compatibility_rejects_other_radio_mismatches() {
    let config = LoraConfig::us915_default();
    let mut notification = startup_notification_without_radio_state(config);
    let (_, bandwidth) = notification
        .commands
        .iter_mut()
        .find(|(command, _)| *command == CMD_BANDWIDTH)
        .expect("bandwidth response");
    *bandwidth = (config.bandwidth_hz + 1).to_be_bytes().to_vec();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);
    monitor.accept_notification(&notification).expect("accept startup responses");

    let error = monitor
        .validate_startup_deadline()
        .expect_err("bandwidth mismatch must remain fatal");

    assert!(error.contains("rnode bandwidth mismatch"));
    assert!(!monitor.online());
    assert_eq!(
        monitor.runtime_status_json("ble://wrong-rnode")["startup_compatibility_warning"],
        serde_json::Value::Null
    );
}

#[test]
fn startup_compatibility_rejects_missing_frequency_response() {
    let config = LoraConfig::us915_default();
    let mut notification = startup_notification_without_radio_state(config);
    notification.commands.retain(|(command, _)| *command != CMD_FREQUENCY);
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);
    monitor.accept_notification(&notification).expect("accept startup responses");

    let error = monitor
        .validate_startup_deadline()
        .expect_err("missing frequency response must remain fatal");

    assert!(error.contains("rnode frequency response is missing"));
    assert!(!monitor.online());
    assert!(!monitor.startup_validated());
}

#[test]
fn payload_writes_wait_for_validated_radio_startup() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);

    assert!(!rnode_ble_payload_writes_enabled(true, Some(&monitor)));
    assert!(!monitor.startup_validated());
    monitor
        .accept_notification(&startup_notification_without_radio_state(config))
        .expect("accept startup responses");
    monitor
        .validate_startup_deadline()
        .expect("compatible startup should validate");

    assert!(monitor.startup_validated());
    assert!(rnode_ble_payload_writes_enabled(true, Some(&monitor)));
    assert!(rnode_ble_payload_writes_enabled(true, None));
    assert!(!rnode_ble_payload_writes_enabled(false, None));
}

#[test]
fn fallback_configuration_keeps_payload_writes_blocked_without_validation() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);

    let error = monitor
        .validate_startup_deadline()
        .expect_err("fallback without probe responses must fail closed");

    assert!(error.contains("detect"));
    assert!(!monitor.startup_validated());
    assert!(!rnode_ble_payload_writes_enabled(true, Some(&monitor)));
    assert!(!rnode_ble_payload_writes_enabled(false, Some(&monitor)));
}

#[test]
fn payload_writes_wait_when_radio_state_arrives_before_startup_validation() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);
    monitor
        .accept_notification(&RnodeBleNotification {
            commands: vec![(CMD_RADIO_STATE, vec![RADIO_STATE_ON])],
            packets: Vec::new(),
        })
        .expect("accept radio-state response");

    assert!(monitor.online());
    assert!(!monitor.startup_validated());
    assert!(!rnode_ble_payload_writes_enabled(true, Some(&monitor)));

    let mut mismatched = startup_notification_without_radio_state(config);
    let (_, bandwidth) = mismatched
        .commands
        .iter_mut()
        .find(|(command, _)| *command == CMD_BANDWIDTH)
        .expect("bandwidth response");
    *bandwidth = (config.bandwidth_hz + 1).to_be_bytes().to_vec();
    monitor.accept_notification(&mismatched).expect("accept startup responses");

    let error = monitor
        .validate_startup_deadline()
        .expect_err("mismatched startup response must remain fatal");
    assert!(error.contains("rnode bandwidth mismatch"));
    assert!(!monitor.startup_validated());
    assert!(!rnode_ble_payload_writes_enabled(true, Some(&monitor)));
}

#[test]
fn startup_compatibility_rejects_malformed_radio_state_response() {
    let config = LoraConfig::us915_default();
    let mut monitor = RnodeBleCommandMonitor::new(config, Duration::ZERO);
    let mut notification = startup_notification_without_radio_state(config);
    notification.commands.push((CMD_RADIO_STATE, Vec::new()));
    monitor
        .accept_notification(&notification)
        .expect("malformed radio-state response should not abort notification handling");

    let error = monitor
        .validate_startup_deadline()
        .expect_err("malformed radio-state response must not use compatibility mode");
    assert!(error.contains("rnode radio state response is missing"));
    assert!(!monitor.startup_validated());
    assert!(!rnode_ble_payload_writes_enabled(true, Some(&monitor)));
}

#[tokio::test]
async fn startup_caps_max_write_len_to_negotiated_att_payload() {
    let backend =
        TestBackend { negotiated_mtu: Some(23), notifications: VecDeque::new(), writes: Vec::new() };
    let config = RnodeBleKissConfig {
        mtu: 508,
        max_write_len: 512,
        ..RnodeBleKissConfig::default()
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.startup().await.expect("startup should succeed");

    assert_eq!(
        runtime.session.config.max_write_len,
        20,
        "startup should clamp writes to the negotiated ATT payload length"
    );
}

#[tokio::test]
async fn startup_preserves_a_conservative_configured_write_cap() {
    let backend = TestBackend {
        negotiated_mtu: Some(517),
        notifications: VecDeque::new(),
        writes: Vec::new(),
    };
    let config = RnodeBleKissConfig {
        mtu: 508,
        max_write_len: 20,
        ..RnodeBleKissConfig::default()
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.startup().await.expect("startup should succeed");

    assert_eq!(
        runtime.session.config.max_write_len, 20,
        "a large negotiated ATT payload must not override the configured firmware-safe write cap"
    );
}

#[tokio::test]
async fn shutdown_can_prefix_display_disable_for_display_capable_rnode() {
    let mut monitor =
        RnodeBleCommandMonitor::new(LoraConfig::us915_default(), Duration::from_secs(1));
    monitor
        .accept_notification(&RnodeBleNotification {
            commands: vec![
                (CMD_DETECT, vec![DETECT_RESP]),
                (CMD_FW_VERSION, vec![1, 52]),
                (CMD_PLATFORM, vec![PLATFORM_ESP32]),
                (CMD_MCU, vec![1]),
            ],
            packets: Vec::new(),
        })
        .expect("accept display-capable probe commands");
    let prefix = monitor.external_framebuffer_frame(false).into_iter().collect::<Vec<_>>();

    let backend = TestBackend::default();
    let config = RnodeBleKissConfig {
        shutdown_frames: LoraConfig::us915_default().shutdown_frames(),
        ..RnodeBleKissConfig::default()
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.shutdown_with_prefix_frames(prefix).await.expect("shutdown writes");
    let backend = runtime.into_backend();
    let raw = backend.writes.into_iter().flat_map(|write| write.payload).collect::<Vec<_>>();
    let frames = decode_frames(&raw, 512).expect("decode shutdown writes");

    assert_eq!(
        frames,
        vec![
            KissFrame::Command(KissCommand::Unknown(CMD_FB_EXT, vec![0])),
            KissFrame::Command(KissCommand::Unknown(CMD_RADIO_STATE, vec![RADIO_STATE_OFF])),
            KissFrame::Command(KissCommand::Unknown(CMD_LEAVE, vec![0xff])),
        ]
    );
}

#[tokio::test]
async fn management_frame_write_uses_existing_ble_kiss_chunking() {
    let backend = TestBackend::default();
    let config = RnodeBleKissConfig {
        max_write_len: 2,
        ..RnodeBleKissConfig::default()
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime
        .send_management_frame(LoraConfig::blink_frame(0x03))
        .await
        .expect("management write");

    let backend = runtime.into_backend();
    let chunks = backend.writes.into_iter().map(|write| write.payload).collect::<Vec<_>>();
    assert!(
        chunks.len() > 1,
        "small max_write_len should force management frame fragmentation"
    );
    let raw = chunks.into_iter().flatten().collect::<Vec<_>>();
    let frames = decode_frames(&raw, 512).expect("decode management write");

    assert_eq!(frames, vec![KissFrame::Command(KissCommand::Unknown(CMD_BLINK, vec![0x03]))]);
}

#[tokio::test]
async fn management_frame_write_queues_radio_state_query() {
    let backend = TestBackend::default();
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    runtime
        .send_management_frame(LoraConfig::radio_state_query_frame())
        .await
        .expect("management write");

    let backend = runtime.into_backend();
    let raw = backend.writes.into_iter().flat_map(|write| write.payload).collect::<Vec<_>>();
    let frames = decode_frames(&raw, 512).expect("decode management write");

    assert_eq!(
        frames,
        vec![KissFrame::Command(KissCommand::Unknown(CMD_RADIO_STATE, vec![RADIO_STATE_ASK]))]
    );
}

#[tokio::test]
async fn native_rnode_ble_management_handle_queues_frames() {
    let iface = NativeRnodeBleKissInterface::new(
        "rnode-ble-test",
        NativeRnodeBleSettings::for_peripheral("RNode Test"),
        RnodeBleKissConfig::default(),
    );
    let handle = iface.rnode_management_handle();

    handle
        .try_dispatch_frame(LoraConfig::blink_frame(0x04))
        .expect("queue management frame");

    let mut rx = iface.management_frame_rx.lock().await;
    let frame = rx.recv().await.expect("management frame queued");
    let frames = decode_frames(&frame, 512).expect("decode queued management frame");

    assert_eq!(frames, vec![KissFrame::Command(KissCommand::Unknown(CMD_BLINK, vec![0x04]))]);
}
