impl RnodeBleCommandMonitor {
    #[must_use]
    pub fn new(config: LoraConfig, startup_response_timeout: Duration) -> Self {
        let mut lora = LoraInterface::new_tcp("ble://rnode", config);
        lora.begin_startup_response_collection();
        Self { lora, startup_deadline: Some(Instant::now() + startup_response_timeout) }
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
                (Ok(_), _) => {}
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

    pub fn accept_degraded_startup(&mut self) {
        self.startup_deadline = None;
    }

    #[must_use]
    pub fn online(&self) -> bool {
        self.lora.online()
    }

    #[must_use]
    pub fn reported_bitrate_bps(&self) -> Option<f64> {
        self.lora.reported_bitrate_bps()
    }

    pub fn validate_startup_deadline(&mut self) -> Result<(), String> {
        let Some(deadline) = self.startup_deadline else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.startup_deadline = None;
        self.lora.validate_startup_responses()
    }
}

#[derive(Debug, Clone)]
pub struct RnodeBleKissSession {
    config: RnodeBleKissConfig,
    decoder: KissStreamDecoder,
    subscribed: bool,
    interface_ready: bool,
    last_read_at: Instant,
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
        self.config
            .shutdown_frames
            .iter()
            .cloned()
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn enqueue_packet(&mut self, payload: &[u8]) -> Vec<RnodeBleWrite> {
        if self.config.kiss.flow_control && !self.interface_ready {
            self.pending_payloads.push_back(payload.to_vec());
            return Vec::new();
        }

        let writes = self.kiss_writes(encode_data_frame(payload));
        if self.config.kiss.flow_control {
            self.interface_ready = false;
        }
        writes
    }

    #[must_use]
    pub fn id_beacon_write(&self) -> Option<RnodeBleWrite> {
        self.config.kiss.id_beacon.as_ref().and_then(|beacon| {
            self.kiss_writes(encode_data_frame(&beacon.payload())).into_iter().next()
        })
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

        let writes = self.kiss_writes(encode_data_frame(&payload));
        if self.config.kiss.flow_control {
            self.interface_ready = false;
        }
        writes
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
        if frames.is_empty()
            && payload.len() == DEFAULT_ATT_NOTIFICATION_PAYLOAD_BYTES
            && self.decoder.has_partial_frame()
        {
            return Err(RnodeBleKissError::Backend {
                operation: "accept_notification_events",
                message: "incomplete 20-byte RNode BLE notification; likely ATT MTU 23 / 20-byte notification payload, and btleplug cannot request a larger MTU before notifications in this backend".to_string(),
            });
        }
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
                    self.interface_ready = true;
                    self.flush_pending_payloads();
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

    fn flush_pending_payloads(&mut self) {
        while self.interface_ready {
            let Some(payload) = self.pending_payloads.pop_front() else {
                break;
            };
            self.pending_writes.extend(self.kiss_writes(encode_data_frame(&payload)));
            if self.config.kiss.flow_control {
                self.interface_ready = false;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{RnodeBleKissConfig, RnodeBleKissError, RnodeBleKissSession};

    #[test]
    fn rnode_ble_notification_reports_likely_default_att_mtu_cap() {
        let mut session = RnodeBleKissSession::new(RnodeBleKissConfig::default());
        let err = session
            .accept_notification_events(&[0x42; 20])
            .expect_err("20-byte incomplete notification should be diagnosed");

        match err {
            RnodeBleKissError::Backend { operation, message } => {
                assert_eq!(operation, "accept_notification_events");
                assert!(message.contains("likely ATT MTU 23"));
                assert!(message.contains("btleplug cannot request a larger MTU"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
