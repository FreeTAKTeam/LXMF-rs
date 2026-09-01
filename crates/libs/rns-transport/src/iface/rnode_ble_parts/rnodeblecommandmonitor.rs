impl RnodeBleCommandMonitor {
    #[must_use]
    pub fn new(config: LoraConfig, startup_response_timeout: Duration) -> Self {
        let mut lora = LoraInterface::new_tcp("ble://rnode", config);
        lora.begin_startup_response_collection();
        Self {
            lora,
            startup_deadline: Some(Instant::now() + startup_response_timeout),
            startup_validated: false,
            startup_payload_writes_enabled: false,
            startup_compatibility_warning: None,
        }
    }

    pub fn accept_notification(
        &mut self,
        notification: &RnodeBleNotification,
    ) -> Result<(), String> {
        for (command, payload) in &notification.commands {
            let result = self.lora.record_command_response(*command, payload);
            let fatal = match &result {
                Ok(_) => false,
                Err(err) => self.lora.last_command_error() == Some(err.as_str()),
            };
            match (result, fatal) {
                (Ok(_), _) => {
                    if matches!(*command, CMD_STAT_RX | CMD_STAT_TX) && payload.len() == 4 {
                        let value = u32::from_be_bytes([
                            payload[0], payload[1], payload[2], payload[3],
                        ]);
                        let counter = if *command == CMD_STAT_RX { "stat_rx" } else { "stat_tx" };
                        log::info!("RNode radio counter counter={counter} value={value}");
                    }
                }
                (Err(err), true) => return Err(err),
                (Err(err), false) => {
                    log::warn!(
                        "ignored malformed RNode BLE command response command=0x{:02x} err={}",
                        command,
                        err
                    );
                }
            }
        }
        for _ in &notification.packets {
            self.lora.record_inbound_data_frame();
        }
        Ok(())
    }

    #[must_use]
    pub fn probe_status(&self) -> RNodeProbeStatus {
        self.lora.probe_status()
    }

    #[must_use]
    pub fn radio_status(&self) -> RNodeRadioStatus {
        self.lora.radio_status()
    }

    #[must_use]
    pub fn hardware_errors(&self) -> &[RNodeHardwareError] {
        self.lora.hardware_errors()
    }

    #[must_use]
    pub fn last_command_error(&self) -> Option<&str> {
        self.lora.last_command_error()
    }

    #[must_use]
    pub fn is_detected(&self) -> bool {
        self.lora.is_detected()
    }

    pub fn reset_startup_deadline(&mut self, timeout: Duration) {
        self.startup_deadline = Some(Instant::now() + timeout);
    }

    #[must_use]
    pub fn startup_validated(&self) -> bool {
        self.startup_validated
    }

    #[must_use]
    fn startup_payload_writes_enabled(&self) -> bool {
        self.startup_payload_writes_enabled
    }

    #[must_use]
    pub fn online(&self) -> bool {
        self.lora.online()
    }

    #[must_use]
    pub fn reported_bitrate_bps(&self) -> Option<f64> {
        self.lora.reported_bitrate_bps()
    }

    #[must_use]
    pub fn external_framebuffer_frame(&self, enable: bool) -> Option<Vec<u8>> {
        self.lora.probe_status().external_framebuffer_frame(enable)
    }

    #[must_use]
    pub fn runtime_status_json(&self, endpoint: &str) -> serde_json::Value {
        let mut value = rnode_ble_runtime_status_json(&self.lora, endpoint);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "startup_validated".to_string(),
                serde_json::Value::Bool(self.startup_validated),
            );
            object.insert(
                "startup_compatibility_warning".to_string(),
                self.startup_compatibility_warning
                    .as_ref()
                    .map_or(serde_json::Value::Null, |warning| {
                        serde_json::Value::String(warning.clone())
                    }),
            );
        }
        value
    }

    pub fn validate_startup_deadline(&mut self) -> Result<(), String> {
        let Some(deadline) = self.startup_deadline else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.startup_deadline = None;
        let validation = match self.lora.validate_startup_responses() {
            Ok(()) => Ok(()),
            Err(error) => match self.lora.accept_missing_radio_state_compatibility() {
                Ok(true) => {
                    let warning = "RNode omitted the startup radio-state response; accepted after all other probe and radio parameters validated".to_string();
                    log::warn!("{warning}");
                    self.startup_compatibility_warning = Some(warning);
                    Ok(())
                }
                Ok(false) | Err(_) => Err(error),
            },
        };
        if validation.is_ok() {
            self.startup_validated = true;
            self.startup_payload_writes_enabled = true;
        }
        validation
    }
}

pub(crate) fn rnode_ble_payload_writes_enabled(
    radio_config_sent: bool,
    command_monitor: Option<&RnodeBleCommandMonitor>,
) -> bool {
    radio_config_sent
        && command_monitor.is_none_or(RnodeBleCommandMonitor::startup_payload_writes_enabled)
}

#[must_use]
pub fn rnode_ble_initial_runtime_status_json(
    config: LoraConfig,
    endpoint: &str,
) -> serde_json::Value {
    let mut lora = LoraInterface::new_tcp(endpoint.to_string(), config);
    lora.begin_startup_response_collection();
    rnode_ble_runtime_status_json(&lora, endpoint)
}

#[must_use]
pub fn rnode_ble_runtime_status_json(
    lora: &LoraInterface,
    endpoint: &str,
) -> serde_json::Value {
    let mut value = lora.runtime_status_json();
    if let Some(object) = value.as_object_mut() {
        object.insert("endpoint".to_string(), serde_json::Value::String(endpoint.to_string()));
        object.insert("bearer".to_string(), serde_json::Value::String("ble".to_string()));
        object.insert("baud_rate".to_string(), serde_json::Value::Null);
        object.insert(
            "startup_compatibility_warning".to_string(),
            serde_json::Value::Null,
        );
    }
    value
}

#[derive(Clone)]
pub struct RnodeBleRuntimeStatusHandle {
    inner: Arc<Mutex<serde_json::Value>>,
}

impl RnodeBleRuntimeStatusHandle {
    #[must_use]
    pub fn new(inner: Arc<Mutex<serde_json::Value>>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("RNode BLE status mutex poisoned").clone()
    }
}

#[cfg(feature = "rnode-ble")]
fn rnode_ble_management_channel(
) -> (RnodeBleManagementFrameSender, RnodeBleManagementFrameReceiver) {
    let (tx, rx) = tokio::sync::mpsc::channel(RNODE_BLE_MANAGEMENT_CHANNEL_CAPACITY);
    (tx, Arc::new(tokio::sync::Mutex::new(rx)))
}

#[cfg(feature = "rnode-ble")]
#[derive(Debug, Clone)]
pub struct RnodeBleManagementHandle {
    tx: RnodeBleManagementFrameSender,
}

#[cfg(feature = "rnode-ble")]
impl RnodeBleManagementHandle {
    pub fn try_dispatch_frame(
        &self,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<Vec<u8>>> {
        self.tx.try_send(frame)
    }

    pub async fn dispatch_frame(
        &self,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.tx.send(frame).await
    }
}

#[derive(Debug, Clone)]
pub struct RnodeBleKissSession {
    config: RnodeBleKissConfig,
    decoder: KissStreamDecoder,
    subscribed: bool,
    interface_ready: bool,
    last_read_at: Instant,
    last_queue_admission_probe_at: Option<Instant>,
    pending_payloads: VecDeque<Vec<u8>>,
    pending_writes: VecDeque<RnodeBleWrite>,
}

impl RnodeBleKissSession {
    #[must_use]
    pub fn new(config: RnodeBleKissConfig) -> Self {
        Self {
            decoder: KissStreamDecoder::new(config.mtu),
            interface_ready: !config.kiss.flow_control,
            subscribed: false,
            last_read_at: Instant::now(),
            last_queue_admission_probe_at: None,
            pending_payloads: VecDeque::new(),
            pending_writes: VecDeque::new(),
            config,
        }
    }

    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    #[must_use]
    pub fn status(&self) -> RnodeBleKissStatus {
        self.status_with_connection(false)
    }

    fn status_with_connection(&self, connected: bool) -> RnodeBleKissStatus {
        RnodeBleKissStatus {
            connected,
            subscribed: self.subscribed,
            interface_ready: self.interface_ready,
            pending_payloads: self.pending_payloads.len(),
            pending_writes: self.pending_writes.len(),
        }
    }

    #[must_use]
    pub fn pending_payloads(&self) -> usize {
        self.pending_payloads.len()
    }

    #[must_use]
    pub fn mtu(&self) -> usize {
        self.config.mtu
    }

    #[must_use]
    pub fn startup_frames(&mut self) -> Vec<RnodeBleWrite> {
        self.subscribed = true;
        if self.config.kiss.flow_control {
            self.last_queue_admission_probe_at = Some(Instant::now());
        }
        self.config
            .kiss
            .command_frames()
            .into_iter()
            .chain(self.config.initial_frames.iter().cloned())
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn deferred_frames(&mut self) -> Vec<RnodeBleWrite> {
        self.config
            .deferred_frames
            .iter()
            .cloned()
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn shutdown_frames(&self) -> Vec<RnodeBleWrite> {
        self.shutdown_frames_with_prefix(std::iter::empty::<Vec<u8>>())
    }

    #[must_use]
    pub fn shutdown_frames_with_prefix(
        &self,
        prefix_frames: impl IntoIterator<Item = Vec<u8>>,
    ) -> Vec<RnodeBleWrite> {
        prefix_frames
            .into_iter()
            .chain(self.config.shutdown_frames.iter().cloned())
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn enqueue_packet(&mut self, payload: &[u8]) -> Vec<RnodeBleWrite> {
        if self.config.kiss.flow_control && !self.interface_ready {
            self.pending_payloads.push_back(payload.to_vec());
            return Vec::new();
        }

        let mut writes = self.kiss_writes(encode_data_frame(payload));
        if self.config.kiss.flow_control {
            self.interface_ready = false;
            writes.extend(self.queue_admission_probe_writes());
        }
        writes
    }

    pub fn id_beacon_write(&self) -> Result<Option<RnodeBleWrite>, &'static str> {
        let Some(beacon) = self.config.kiss.id_beacon.as_ref() else {
            return Ok(None);
        };
        self.kiss_writes(encode_data_frame(&beacon.payload()))
            .into_iter()
            .next()
            .ok_or("id beacon produced no BLE writes")
            .map(Some)
    }

    #[must_use]
    pub fn enqueue_id_beacon(&mut self) -> Vec<RnodeBleWrite> {
        let Some(beacon) = self.config.kiss.id_beacon.as_ref() else {
            return Vec::new();
        };
        let payload = beacon.payload();
        if self.config.kiss.flow_control && !self.interface_ready {
            self.pending_payloads.push_back(payload);
            return Vec::new();
        }

        let mut writes = self.kiss_writes(encode_data_frame(&payload));
        if self.config.kiss.flow_control {
            self.interface_ready = false;
            writes.extend(self.queue_admission_probe_writes());
        }
        writes
    }

    #[must_use]
    pub fn management_frame_writes(&self, frame: Vec<u8>) -> Vec<RnodeBleWrite> {
        self.kiss_writes(frame)
    }

    pub fn accept_notification(
        &mut self,
        payload: &[u8],
    ) -> Result<Vec<Vec<u8>>, RnodeBleKissError> {
        Ok(self.accept_notification_events(payload)?.packets)
    }

    pub fn accept_notification_events(
        &mut self,
        payload: &[u8],
    ) -> Result<RnodeBleNotification, RnodeBleKissError> {
        if self.decoder.has_partial_frame()
            && self.last_read_at.elapsed() >= self.config.read_frame_timeout
        {
            self.decoder.clear_partial_frame();
        }
        self.last_read_at = Instant::now();
        let frames = self.decoder.push_bytes(payload)?;
        let mut notification = RnodeBleNotification::default();
        for frame in frames {
            match frame {
                KissFrame::Data(payload) => {
                    let is_id_beacon = self
                        .config
                        .kiss
                        .id_beacon
                        .as_ref()
                        .is_some_and(|beacon| beacon.matches_payload(&payload));
                    if !is_id_beacon {
                        notification.packets.push(payload);
                    }
                }
                KissFrame::Command(KissCommand::Ready) => {
                    if self.config.kiss.flow_control {
                        self.interface_ready = true;
                        self.flush_pending_payloads();
                    }
                }
                KissFrame::Command(KissCommand::Unknown(CMD_READY, payload))
                    if payload.first().copied() == Some(0) =>
                {
                    if self.config.kiss.flow_control {
                        self.interface_ready = false;
                    }
                }
                KissFrame::Command(KissCommand::Unknown(command, payload)) => {
                    notification.commands.push((command, payload));
                }
            }
        }
        Ok(notification)
    }

    #[must_use]
    pub fn take_pending_writes(&mut self) -> Vec<RnodeBleWrite> {
        self.pending_writes.drain(..).collect()
    }

    #[must_use]
    pub fn queue_admission_probe_if_due(&mut self) -> Vec<RnodeBleWrite> {
        if !self.config.kiss.flow_control
            || self.interface_ready
            || self.pending_payloads.is_empty()
            || self.last_queue_admission_probe_at.is_some_and(|last| {
                last.elapsed() < RNODE_QUEUE_ADMISSION_POLL_INTERVAL
            })
        {
            return Vec::new();
        }
        self.queue_admission_probe_writes()
    }

    fn flush_pending_payloads(&mut self) {
        while self.interface_ready {
            let Some(payload) = self.pending_payloads.pop_front() else {
                break;
            };
            self.pending_writes.extend(self.kiss_writes(encode_data_frame(&payload)));
            if self.config.kiss.flow_control {
                self.interface_ready = false;
                let probe_writes = self.queue_admission_probe_writes();
                self.pending_writes.extend(probe_writes);
            }
        }
    }

    fn queue_admission_probe_writes(&mut self) -> Vec<RnodeBleWrite> {
        self.last_queue_admission_probe_at = Some(Instant::now());
        self.kiss_writes(crate::kiss::encode_command_frame(crate::kiss::CMD_READY, &[1]))
    }

    fn kiss_writes(&self, payload: Vec<u8>) -> Vec<RnodeBleWrite> {
        let chunk_len = self.config.max_write_len.max(1);
        payload
            .chunks(chunk_len)
            .map(|chunk| RnodeBleWrite {
                characteristic_uuid: self.config.write_characteristic_uuid,
                with_response: self.config.write_with_response,
                payload: chunk.to_vec(),
            })
            .collect()
    }
}
