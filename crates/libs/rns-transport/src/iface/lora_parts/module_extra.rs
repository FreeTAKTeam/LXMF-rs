async fn run_lora_kiss_stream<IO>(stream: IO, run: LoraStreamRun)
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let stream_cancel = run.cancel.child_token();
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
    let (data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
    let probe_status_task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
        run.interface,
        command_rx,
        data_rx,
        stream_cancel.clone(),
        Some(run.startup_response_timeout),
    ));

    run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: run.iface_address,
            device: run.endpoint_label,
            mtu: usize::from(run.config.max_payload_bytes),
            flow_control: run.flow_control,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: run.config.command_frames(),
            shutdown_frames: run.config.shutdown_frames(),
            id_beacon: run.id_beacon,
            activity_probe: run.activity_probe,
            payload_adapter: KissPayloadAdapter::Raw,
            strip_command_port_nibble: false,
            command_tx: Some(command_tx),
            data_rx_tx: Some(data_rx_tx),
            management_frame_rx: Some(run.management_frame_rx),
            runtime_status: None,
        },
        stream_cancel,
        run.rx_channel,
        run.tx_channel,
    )
    .await;
    if let Err(err) = probe_status_task.await {
        if !err.is_cancelled() {
            log::warn!("LoRa probe status task failed iface={} err={}", run.iface_address, err);
        }
    }
}

impl Interface for LoraInterface {
    fn mtu() -> usize {
        220
    }

    fn configured_mtu(&self) -> usize {
        usize::from(self.config.max_payload_bytes)
    }
}

async fn record_probe_status_commands_with_startup_timeout(
    interface: Arc<std::sync::Mutex<LoraInterface>>,
    mut command_rx: tokio::sync::mpsc::Receiver<KissCommandFrame>,
    mut data_rx: tokio::sync::mpsc::Receiver<()>,
    cancel: tokio_util::sync::CancellationToken,
    startup_response_timeout: Option<Duration>,
) {
    if startup_response_timeout.is_some() {
        let mut guard = interface.lock().expect("lora interface mutex poisoned");
        guard.begin_startup_response_collection();
    }
    let mut startup_deadline =
        startup_response_timeout.map(|timeout| Box::pin(tokio::time::sleep(timeout)));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = async {
                if let Some(deadline) = startup_deadline.as_mut() {
                    deadline.as_mut().await;
                }
            }, if startup_deadline.is_some() => {
                startup_deadline = None;
                let result = {
                    let mut guard = interface.lock().expect("lora interface mutex poisoned");
                    match guard.validate_startup_responses() {
                        Ok(()) => Ok(()),
                        Err(err) => {
                            guard.last_command_error = Some(err.clone());
                            Err(err)
                        }
                    }
                };
                match result {
                    Ok(()) => log::debug!("validated LoRa RNode startup responses"),
                    Err(err) => {
                        log::warn!("LoRa RNode startup response validation failed err={}", err);
                        cancel.cancel();
                        break;
                    }
                }
            }
            command = command_rx.recv() => {
                let Some(command) = command else {
                    break;
                };
                let result = {
                    let mut guard = interface.lock().expect("lora interface mutex poisoned");
                    let result = guard.record_command_response(command.command, &command.payload);
                    let fatal = match &result {
                        Ok(_) => false,
                        Err(err) => guard.last_command_error() == Some(err.as_str()),
                    };
                    (result, fatal)
                };
                match result {
                    (Ok(true), _) => log::debug!(
                        "recorded LoRa RNode command response command=0x{:02x}",
                        command.command
                    ),
                    (Ok(false), _) => {}
                    (Err(err), true) => {
                        log::warn!(
                            "fatal LoRa RNode command response command=0x{:02x} err={}",
                            command.command,
                            err
                        );
                        cancel.cancel();
                        break;
                    }
                    (Err(err), false) => log::warn!(
                        "ignored malformed LoRa RNode probe response command=0x{:02x} err={}",
                        command.command,
                        err
                    ),
                }
            }
            data = data_rx.recv() => {
                if data.is_none() {
                    break;
                }
                let mut guard = interface.lock().expect("lora interface mutex poisoned");
                guard.record_inbound_data_frame();
            }
        }
    }
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

fn preflight_tcp_connect(addr: &str) -> Result<(), String> {
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| format!("lora tcp preflight resolve failed addr={addr} err={err}"))?
        .next()
        .ok_or_else(|| format!("lora tcp preflight resolve failed addr={addr}"))?;
    StdTcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
        .map(|_| ())
        .map_err(|err| format!("lora tcp preflight connect failed addr={addr} err={err}"))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use super::*;

    #[tokio::test]
    async fn command_response_task_cancels_stream_on_fatal_rnode_error() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            None,
        ));

        command_tx
            .send(KissCommandFrame { command: CMD_ERROR, payload: vec![ERROR_TXFAILED] })
            .await
            .expect("send fatal command");

        tokio::time::timeout(Duration::from_secs(1), cancel.cancelled())
            .await
            .expect("fatal RNode command should cancel the stream");

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), Some("Hardware transmit failure"));
    }

    #[tokio::test]
    async fn command_response_task_keeps_stream_on_malformed_rnode_response() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            None,
        ));

        command_tx
            .send(KissCommandFrame { command: CMD_FW_VERSION, payload: vec![1] })
            .await
            .expect("send malformed command");

        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancel.cancelled()).await.is_err(),
            "malformed RNode response should not cancel the stream"
        );

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), None);
    }

    #[tokio::test]
    async fn command_response_task_clears_signal_stats_on_inbound_data_frame() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        {
            let mut guard = iface.lock().expect("lora interface mutex poisoned");
            guard.record_command_response(CMD_SF, &[9]).expect("spreading factor");
            guard.record_command_response(CMD_STAT_RSSI, &[97]).expect("rssi");
            guard.record_command_response(CMD_STAT_SNR, &[0xF8]).expect("negative snr");
        }

        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            None,
        ));

        data_rx_tx.send(()).await.expect("send data frame event");

        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(command_tx);
        drop(data_rx_tx);
        task.await.expect("command task");

        let status = iface.lock().expect("lora interface mutex poisoned").radio_status();
        assert_eq!(status.rssi_dbm, None);
        assert_eq!(status.snr_db, None);
        assert_eq!(status.signal_quality_percent, Some(57.9));
    }

    #[tokio::test]
    async fn command_response_task_cancels_stream_on_missing_startup_responses_after_deadline() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            Some(Duration::from_millis(10)),
        ));

        tokio::time::timeout(Duration::from_secs(1), cancel.cancelled())
            .await
            .expect("missing startup responses should cancel the stream");

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert!(
            guard.last_command_error().is_some_and(|err| err.contains("detect")),
            "unexpected startup error: {:?}",
            guard.last_command_error()
        );
    }

    #[tokio::test]
    async fn command_response_task_keeps_stream_when_startup_responses_validate_before_deadline() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            Some(Duration::from_millis(200)),
        ));

        for frame in [
            KissCommandFrame { command: CMD_DETECT, payload: vec![DETECT_RESP] },
            KissCommandFrame { command: CMD_FW_VERSION, payload: vec![1, 52] },
            KissCommandFrame { command: CMD_PLATFORM, payload: vec![PLATFORM_ESP32] },
            KissCommandFrame { command: CMD_MCU, payload: vec![0x01] },
            KissCommandFrame {
                command: CMD_FREQUENCY,
                payload: 915_000_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame {
                command: CMD_BANDWIDTH,
                payload: 125_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame { command: CMD_TXPOWER, payload: vec![17] },
            KissCommandFrame { command: CMD_SF, payload: vec![9] },
            KissCommandFrame { command: CMD_CR, payload: vec![5] },
            KissCommandFrame { command: CMD_RADIO_STATE, payload: vec![RADIO_STATE_ON] },
        ] {
            command_tx.send(frame).await.expect("send startup command");
        }

        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancel.cancelled()).await.is_err(),
            "valid startup responses should not cancel the stream before the deadline"
        );

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), None);
        guard.validate_startup_responses().expect("recorded startup responses");
    }

    #[tokio::test]
    async fn command_response_task_clears_stale_startup_state_for_new_stream() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        {
            let mut guard = iface.lock().expect("lora interface mutex poisoned");
            guard.record_command_response(CMD_ERROR, &[ERROR_TXFAILED]).expect_err("fatal error");
            assert_eq!(guard.last_command_error(), Some("Hardware transmit failure"));
        }

        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            Some(Duration::from_millis(200)),
        ));

        for frame in [
            KissCommandFrame { command: CMD_DETECT, payload: vec![DETECT_RESP] },
            KissCommandFrame { command: CMD_FW_VERSION, payload: vec![1, 52] },
            KissCommandFrame { command: CMD_PLATFORM, payload: vec![PLATFORM_ESP32] },
            KissCommandFrame { command: CMD_MCU, payload: vec![0x01] },
            KissCommandFrame {
                command: CMD_FREQUENCY,
                payload: 915_000_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame {
                command: CMD_BANDWIDTH,
                payload: 125_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame { command: CMD_TXPOWER, payload: vec![17] },
            KissCommandFrame { command: CMD_SF, payload: vec![9] },
            KissCommandFrame { command: CMD_CR, payload: vec![5] },
            KissCommandFrame { command: CMD_RADIO_STATE, payload: vec![RADIO_STATE_ON] },
        ] {
            command_tx.send(frame).await.expect("send startup command");
        }

        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancel.cancelled()).await.is_err(),
            "fresh valid startup responses should not inherit stale fatal errors"
        );

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), None);
        guard.validate_startup_responses().expect("fresh startup responses");
    }

    #[tokio::test]
    async fn lora_management_handle_writes_runtime_command_frame_over_duplex() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let handle = {
            let guard = iface.lock().expect("lora interface mutex poisoned");
            guard.rnode_management_handle()
        };
        let management_frame_rx = {
            let guard = iface.lock().expect("lora interface mutex poisoned");
            guard.management_frame_rx.clone()
        };
        let (stream, mut peer) = tokio::io::duplex(4096);
        let cancel = CancellationToken::new();
        let (rx_channel, _rx_recv) = tokio::sync::mpsc::channel(1);
        let (_tx_send, tx_recv) = tokio::sync::mpsc::channel(1);
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_recv));

        let task_cancel = cancel.clone();
        let task = tokio::spawn(run_lora_kiss_stream(
            stream,
            LoraStreamRun {
                interface: iface,
                cancel: task_cancel,
                iface_address: crate::hash::AddressHash::default(),
                endpoint_label: "duplex-rnode".to_string(),
                config: LoraConfig::us915_default(),
                flow_control: false,
                id_beacon: None,
                activity_probe: None,
                startup_response_timeout: Duration::from_secs(60),
                management_frame_rx,
                rx_channel,
                tx_channel,
            },
        ));

        let frame = LoraConfig::blink_frame(0x03);
        handle.blink(0x03).await.expect("queue blink management command");

        let mut seen = Vec::new();
        let mut buffer = [0_u8; 512];
        for _ in 0..12 {
            let read = tokio::time::timeout(
                Duration::from_secs(1),
                tokio::io::AsyncReadExt::read(&mut peer, &mut buffer),
            )
            .await
            .expect("management frame should be written")
            .expect("read management frame bytes");
            if read == 0 {
                break;
            }
            seen.extend_from_slice(&buffer[..read]);
            if seen.windows(frame.len()).any(|window| window == frame.as_slice()) {
                cancel.cancel();
                drop(peer);
                task.await.expect("lora stream exits");
                return;
            }
        }

        cancel.cancel();
        drop(peer);
        task.await.expect("lora stream exits");
        panic!("did not observe blink management frame in stream bytes: {seen:02x?}");
    }

    #[test]
    fn lora_runtime_status_json_exposes_rnode_probe_and_radio_state() {
        let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
        iface.record_command_response(CMD_DETECT, &[DETECT_RESP]).expect("detect");
        iface.record_command_response(CMD_FW_VERSION, &[1, 52]).expect("firmware");
        iface.record_command_response(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("platform");
        iface.record_command_response(CMD_MCU, &[0x01]).expect("mcu");
        iface
            .record_command_response(CMD_FREQUENCY, &915_000_000_u32.to_be_bytes())
            .expect("frequency");
        iface
            .record_command_response(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
            .expect("bandwidth");
        iface.record_command_response(CMD_TXPOWER, &[17]).expect("tx power");
        iface.record_command_response(CMD_SF, &[9]).expect("sf");
        iface.record_command_response(CMD_CR, &[5]).expect("cr");
        iface.record_command_response(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio state");
        iface.record_command_response(CMD_STAT_RX, &7_u32.to_be_bytes()).expect("rx");
        iface.record_command_response(CMD_STAT_TX, &11_u32.to_be_bytes()).expect("tx");

        let json = iface.runtime_status_json();

        assert_eq!(json["endpoint"].as_str(), Some("COM9"));
        assert_eq!(json["bearer"].as_str(), Some("serial"));
        assert_eq!(json["baud_rate"].as_u64(), Some(115_200));
        assert_eq!(json["probe_status"]["detected"].as_bool(), Some(true));
        assert_eq!(json["probe_status"]["firmware_version"]["label"].as_str(), Some("1.52"));
        assert_eq!(json["probe_status"]["has_display"].as_bool(), Some(true));
        assert_eq!(json["radio_status"]["frequency_hz"].as_u64(), Some(915_000_000));
        assert_eq!(json["radio_status"]["bandwidth_hz"].as_u64(), Some(125_000));
        assert_eq!(json["radio_status"]["spreading_factor"].as_u64(), Some(9));
        assert_eq!(json["radio_status"]["coding_rate"].as_u64(), Some(5));
        assert_eq!(json["radio_status"]["tx_power_dbm"].as_u64(), Some(17));
        assert_eq!(json["radio_status"]["radio_state"].as_u64(), Some(RADIO_STATE_ON.into()));
        assert_eq!(json["radio_status"]["stat_rx"].as_u64(), Some(7));
        assert_eq!(json["radio_status"]["stat_tx"].as_u64(), Some(11));
        assert_eq!(json["online"].as_bool(), Some(true));
        assert!(json["last_command_error"].is_null());
    }
}
