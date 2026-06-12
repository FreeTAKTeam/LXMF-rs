#[cfg(feature = "vrn76-kiss-ble")]
fn parse_vrn76_uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("VR-N76 UUID constants must be valid")
}

#[derive(Debug)]
pub struct Vrn76KissBleRuntime<B> {
    backend: B,
    session: Vrn76KissBleSession,
    connected: bool,
    pending_packets: VecDeque<Vec<u8>>,
    startup_write_failures: usize,
}

impl<B> Vrn76KissBleRuntime<B> {
    #[must_use]
    pub fn new(backend: B, config: Vrn76KissBleConfig) -> Self {
        Self {
            backend,
            session: Vrn76KissBleSession::new(config),
            connected: false,
            pending_packets: VecDeque::new(),
            startup_write_failures: 0,
        }
    }

    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    #[must_use]
    pub fn into_backend(self) -> B {
        self.backend
    }

    #[must_use]
    pub fn status(&self) -> Vrn76KissBleStatus {
        self.session.status_with_connection(
            self.connected,
            self.pending_packets.len(),
            self.startup_write_failures,
        )
    }
}

impl<B> Vrn76KissBleRuntime<B>
where
    B: Vrn76KissBleBackend,
{
    pub async fn connect_and_configure(&mut self) -> Result<(), Vrn76KissBleError> {
        self.connected = false;
        self.reset_session_state();
        self.backend
            .connect()
            .await
            .map_err(|message| Vrn76KissBleError::Backend { operation: "connect", message })?;
        self.backend.subscribe_indications().await.map_err(|message| {
            Vrn76KissBleError::Backend { operation: "subscribe_indications", message }
        })?;
        let writes = self.session.startup_frames();
        self.startup_write_failures = self.write_startup_commands(writes).await;
        self.connected = true;
        Ok(())
    }

    fn reset_session_state(&mut self) {
        let config = self.session.config.clone();
        self.session = Vrn76KissBleSession::new(config);
        self.pending_packets.clear();
        self.startup_write_failures = 0;
    }

    pub async fn send_packet(&mut self, payload: &[u8]) -> Result<(), Vrn76KissBleError> {
        if payload.len() > self.session.config.mtu {
            return Err(Vrn76KissBleError::PacketTooLarge {
                limit: self.session.config.mtu,
                actual: payload.len(),
            });
        }
        let writes = self.session.enqueue_packet(payload);
        self.write_all(writes, "write_packet").await
    }

    pub async fn send_id_beacon(&mut self) -> Result<(), Vrn76KissBleError> {
        let writes = self.session.enqueue_id_beacon();
        self.write_all(writes, "write_id_beacon").await
    }

    pub async fn poll_next_packet(&mut self) -> Result<Option<Vec<u8>>, Vrn76KissBleError> {
        if let Some(packet) = self.pending_packets.pop_front() {
            return Ok(Some(packet));
        }

        let Some(indication) = self.backend.next_indication().await.map_err(|message| {
            self.connected = false;
            Vrn76KissBleError::Backend { operation: "next_indication", message }
        })?
        else {
            return Ok(None);
        };

        let mut packets = self.session.accept_indication(&indication)?;
        let writes = self.session.take_pending_writes();
        self.write_all(writes, "ready_write").await?;
        self.pending_packets.extend(packets.drain(..));
        Ok(self.pending_packets.pop_front())
    }

    async fn write_all(
        &mut self,
        writes: Vec<BleWrite>,
        operation: &'static str,
    ) -> Result<(), Vrn76KissBleError> {
        for write in writes {
            self.backend.write(write).await.map_err(|message| {
                self.connected = false;
                Vrn76KissBleError::Backend { operation, message }
            })?;
        }
        Ok(())
    }

    async fn write_startup_commands(&mut self, writes: Vec<BleWrite>) -> usize {
        let mut failures = 0;
        for write in writes {
            if self.backend.write(write).await.is_err() {
                failures += 1;
            }
        }
        failures
    }
}

#[derive(Debug, Clone)]
pub struct Vrn76KissBleSession {
    config: Vrn76KissBleConfig,
    decoder: KissStreamDecoder,
    last_read_at: StdInstant,
    subscribed: bool,
    interface_ready: bool,
    pending_payloads: VecDeque<Vec<u8>>,
    pending_writes: VecDeque<BleWrite>,
    pending_tnc_fragment: Vec<u8>,
    next_tnc_fragment_id: u8,
    pending_tnc_channel_id: Option<u8>,
}

impl Vrn76KissBleSession {
    #[must_use]
    pub fn new(config: Vrn76KissBleConfig) -> Self {
        Self {
            decoder: KissStreamDecoder::new(config.mtu),
            last_read_at: StdInstant::now(),
            interface_ready: !config.kiss.flow_control,
            subscribed: false,
            pending_payloads: VecDeque::new(),
            pending_writes: VecDeque::new(),
            pending_tnc_fragment: Vec::new(),
            next_tnc_fragment_id: 0,
            pending_tnc_channel_id: None,
            config,
        }
    }

    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    #[must_use]
    pub fn status(&self) -> Vrn76KissBleStatus {
        self.status_with_connection(false, 0, 0)
    }

    fn status_with_connection(
        &self,
        connected: bool,
        pending_packets: usize,
        startup_write_failures: usize,
    ) -> Vrn76KissBleStatus {
        Vrn76KissBleStatus {
            connected,
            subscribed: self.subscribed,
            interface_ready: self.interface_ready,
            startup_write_failures,
            pending_payloads: self.pending_payloads.len(),
            pending_writes: self.pending_writes.len(),
            pending_packets,
        }
    }

    #[must_use]
    pub fn startup_frames(&mut self) -> Vec<BleWrite> {
        self.subscribed = true;
        self.config
            .kiss
            .command_frames()
            .into_iter()
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn enqueue_packet(&mut self, payload: &[u8]) -> Vec<BleWrite> {
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
    pub fn id_beacon_write(&self) -> Option<BleWrite> {
        self.config.kiss.id_beacon.as_ref().and_then(|beacon| {
            self.kiss_writes(encode_data_frame(&beacon.payload())).into_iter().next()
        })
    }

    #[must_use]
    pub fn enqueue_id_beacon(&mut self) -> Vec<BleWrite> {
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

    pub fn accept_indication(&mut self, payload: &[u8]) -> Result<Vec<Vec<u8>>, Vrn76KissBleError> {
        let kiss_payload = match self.config.frame_mode {
            Vrn76FrameMode::BenshiTncData => {
                let Some(kiss_payload) = self.accept_benshi_data_rxd_event(payload)? else {
                    return Ok(Vec::new());
                };
                kiss_payload
            }
            Vrn76FrameMode::RawKiss => payload.to_vec(),
        };
        if self.decoder.has_partial_frame()
            && self.last_read_at.elapsed() >= self.config.read_frame_timeout
        {
            self.decoder.clear_partial_frame();
        }
        self.last_read_at = StdInstant::now();
        let frames = self.decoder.push_bytes(&kiss_payload)?;
        let mut packets = Vec::new();
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
                        packets.push(payload);
                    }
                }
                KissFrame::Command(KissCommand::Ready) => {
                    self.interface_ready = true;
                    self.flush_pending_payloads();
                }
                KissFrame::Command(KissCommand::Unknown(_, _)) => {}
            }
        }
        Ok(packets)
    }

    fn accept_benshi_data_rxd_event(
        &mut self,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, Vrn76KissBleError> {
        let fragment = match decode_benshi_data_rxd_event(payload) {
            Ok(fragment) => fragment,
            Err(err) => {
                self.reset_tnc_fragment_state();
                return Err(err);
            }
        };
        if fragment.fragment_id != self.next_tnc_fragment_id {
            let expected_fragment_id = self.next_tnc_fragment_id;
            self.reset_tnc_fragment_state();
            return Err(Vrn76KissBleError::UnexpectedTncFragment {
                expected_fragment_id,
                actual_fragment_id: fragment.fragment_id,
            });
        }

        if fragment.fragment_id == 0 {
            self.pending_tnc_channel_id = fragment.channel_id;
        } else if self.pending_tnc_channel_id != fragment.channel_id {
            let expected_channel_id = self.pending_tnc_channel_id;
            self.reset_tnc_fragment_state();
            return Err(Vrn76KissBleError::UnexpectedTncChannel {
                expected_channel_id,
                actual_channel_id: fragment.channel_id,
            });
        }

        self.pending_tnc_fragment.extend_from_slice(fragment.payload);
        if fragment.is_final {
            let kiss_payload = std::mem::take(&mut self.pending_tnc_fragment);
            self.reset_tnc_fragment_state();
            return Ok(Some(kiss_payload));
        }

        self.next_tnc_fragment_id = self.next_tnc_fragment_id.saturating_add(1);
        Ok(None)
    }

    fn reset_tnc_fragment_state(&mut self) {
        self.pending_tnc_fragment.clear();
        self.next_tnc_fragment_id = 0;
        self.pending_tnc_channel_id = None;
    }

    #[must_use]
    pub fn take_pending_writes(&mut self) -> Vec<BleWrite> {
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

    fn kiss_writes(&self, kiss_payload: Vec<u8>) -> Vec<BleWrite> {
        match self.config.frame_mode {
            Vrn76FrameMode::BenshiTncData => self.benshi_writes(&kiss_payload),
            Vrn76FrameMode::RawKiss => {
                self.raw_kiss_write_chunks(&kiss_payload).map(Self::write_with_response).collect()
            }
        }
    }

    fn raw_kiss_write_chunks<'a>(&self, payload: &'a [u8]) -> impl Iterator<Item = Vec<u8>> + 'a {
        let chunk_len = self.config.max_write_len.max(1);
        payload.chunks(chunk_len).map(<[u8]>::to_vec)
    }

    fn benshi_writes(&self, kiss_payload: &[u8]) -> Vec<BleWrite> {
        let fragment_payload_len = self
            .config
            .max_write_len
            .saturating_sub(BENSHI_MESSAGE_HEADER_LEN + TNC_FRAGMENT_HEADER_LEN)
            .max(1);
        let chunk_count = kiss_payload.len().div_ceil(fragment_payload_len).max(1);
        let mut writes = Vec::with_capacity(chunk_count);
        for (index, chunk) in kiss_payload.chunks(fragment_payload_len).enumerate() {
            writes.push(Self::write_with_response(encode_benshi_ht_send_data_fragment(
                index as u8,
                index + 1 == chunk_count,
                chunk,
            )));
        }
        if kiss_payload.is_empty() {
            writes.push(Self::write_with_response(encode_benshi_ht_send_data_fragment(
                0,
                true,
                &[],
            )));
        }
        writes
    }

    fn write_with_response(payload: Vec<u8>) -> BleWrite {
        BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload,
        }
    }
}

#[must_use]
pub fn encode_benshi_ht_send_data(kiss_payload: &[u8]) -> Vec<u8> {
    encode_benshi_ht_send_data_fragment(0, true, kiss_payload)
}

#[must_use]
pub fn encode_benshi_ht_send_data_fragment(
    fragment_id: u8,
    is_final: bool,
    kiss_payload: &[u8],
) -> Vec<u8> {
    let mut frame = encode_benshi_message(false, BENSHI_COMMAND_HT_SEND_DATA);
    frame.extend_from_slice(&encode_tnc_data_fragment(fragment_id, is_final, kiss_payload));
    frame
}

#[cfg(feature = "vrn76-kiss-ble")]
#[derive(Debug, Clone)]
pub struct NativeVrn76KissBleInterface {
    label: String,
    settings: NativeVrn76BleSettings,
    config: Vrn76KissBleConfig,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
}
