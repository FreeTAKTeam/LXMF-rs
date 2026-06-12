#[cfg(feature = "vrn76-kiss-ble")]
impl NativeVrn76KissBleInterface {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        settings: NativeVrn76BleSettings,
        config: Vrn76KissBleConfig,
    ) -> Self {
        Self {
            label: label.into(),
            settings,
            config,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
        }
    }

    #[must_use]
    pub fn with_reconnect_backoff(mut self, reconnect_backoff: Duration) -> Self {
        self.reconnect_backoff = reconnect_backoff;
        if self.max_reconnect_backoff < self.reconnect_backoff {
            self.max_reconnect_backoff = self.reconnect_backoff;
        }
        self
    }

    #[must_use]
    pub fn with_max_reconnect_backoff(mut self, max_reconnect_backoff: Duration) -> Self {
        self.max_reconnect_backoff = max_reconnect_backoff.max(self.reconnect_backoff);
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (label, settings, config, reconnect_backoff, max_reconnect_backoff) = {
            let guard = context.inner.lock().expect("VR-N76 interface mutex poisoned");
            (
                guard.label.clone(),
                guard.settings.clone(),
                guard.config.clone(),
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
            )
        };
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let backend = NativeVrn76BleBackend::new(settings.clone());
            let mut runtime = Vrn76KissBleRuntime::new(backend, config.clone());
            if let Err(err) = runtime.connect_and_configure().await {
                log::warn!(
                    "VR-N76 KISS-over-BLE session setup failed iface={} addr={} err={:?}",
                    label,
                    iface_address,
                    err
                );
                let mut backend = runtime.into_backend();
                let _ = backend.cleanup().await;
                sleep(active_backoff).await;
                active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
                continue;
            }
            let status = runtime.status();
            if status.startup_write_failures > 0 {
                log::warn!(
                    "VR-N76 KISS-over-BLE startup command write failures iface={} addr={} failures={}",
                    label,
                    iface_address,
                    status.startup_write_failures
                );
            }
            active_backoff = reconnect_backoff;
            log::info!(
                "VR-N76 KISS-over-BLE session established iface={} addr={} peripheral_id={}",
                label,
                iface_address,
                settings.peripheral_id
            );

            let mut tx_buffer = vec![0_u8; config.mtu];
            let mut reconnect_needed = false;
            let mut first_tx_at: Option<Instant> = None;
            while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
                while let Ok(message) = tx_channel.try_recv() {
                    let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                    if message.packet.serialize(&mut output).is_err() {
                        log::warn!("VR-N76 packet serialize failed iface={}", label);
                        continue;
                    }
                    if let Err(err) = runtime.send_packet(output.as_slice()).await {
                        log::warn!("VR-N76 packet write failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                    if first_tx_at.is_none() {
                        first_tx_at = Some(Instant::now());
                    }
                }
                if reconnect_needed {
                    break;
                }

                if let (Some(beacon), Some(first_tx)) =
                    (config.kiss.id_beacon.as_ref(), first_tx_at)
                {
                    if first_tx.elapsed() >= beacon.interval {
                        if let Err(err) = runtime.send_id_beacon().await {
                            log::warn!(
                                "VR-N76 station ID write failed iface={} err={:?}",
                                label,
                                err
                            );
                            reconnect_needed = true;
                            break;
                        }
                        first_tx_at = None;
                    }
                }

                match timeout(Duration::from_millis(100), runtime.poll_next_packet()).await {
                    Ok(Ok(Some(payload))) => {
                        if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&payload)) {
                            let _ = rx_channel
                                .send(RxMessage {
                                    address: iface_address,
                                    packet,
                                    source: IfaceSource::None,
                                })
                                .await;
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(err)) => {
                        log::warn!("VR-N76 packet read failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                }
            }

            let mut backend = runtime.into_backend();
            let _ = backend.cleanup().await;
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }
            if reconnect_needed {
                sleep(active_backoff).await;
                active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
            }
        }

        iface_stop.cancel();
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
impl Interface for NativeVrn76KissBleInterface {
    fn mtu() -> usize {
        564
    }

    fn configured_mtu(&self) -> usize {
        self.config.mtu
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

#[must_use]
pub fn encode_benshi_data_rxd_event(kiss_payload: &[u8]) -> Vec<u8> {
    let mut frame = encode_benshi_message(false, BENSHI_COMMAND_EVENT_NOTIFICATION);
    frame.push(BENSHI_EVENT_DATA_RXD);
    frame.extend_from_slice(&encode_tnc_data_fragment(0, true, kiss_payload));
    frame
}

fn encode_benshi_message(is_reply: bool, command: u16) -> Vec<u8> {
    let command_word = (u16::from(is_reply) << 15) | (command & 0x7fff);
    let mut frame = Vec::with_capacity(4);
    frame.extend_from_slice(&BENSHI_COMMAND_GROUP_BASIC.to_be_bytes());
    frame.extend_from_slice(&command_word.to_be_bytes());
    frame
}

fn encode_tnc_data_fragment(fragment_id: u8, is_final: bool, kiss_payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(kiss_payload.len() + 1);
    frame.push((u8::from(is_final) << 7) | (fragment_id & 0x3f));
    frame.extend_from_slice(kiss_payload);
    frame
}

struct TncDataFragment<'a> {
    fragment_id: u8,
    is_final: bool,
    channel_id: Option<u8>,
    payload: &'a [u8],
}

fn decode_benshi_data_rxd_event(payload: &[u8]) -> Result<TncDataFragment<'_>, Vrn76KissBleError> {
    let (command_group, is_reply, command, body) = decode_benshi_message_header(payload)?;
    if command_group != BENSHI_COMMAND_GROUP_BASIC
        || is_reply
        || command != BENSHI_COMMAND_EVENT_NOTIFICATION
    {
        return Err(Vrn76KissBleError::UnsupportedBenshiMessage { command_group, command });
    }
    let Some((&event_type, event_body)) = body.split_first() else {
        return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
    };
    if event_type != BENSHI_EVENT_DATA_RXD {
        return Err(Vrn76KissBleError::UnsupportedBenshiEvent { event_type });
    }
    decode_tnc_data_fragment(event_body)
}

fn decode_benshi_message_header(
    payload: &[u8],
) -> Result<(u16, bool, u16, &[u8]), Vrn76KissBleError> {
    if payload.len() < 4 {
        return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
    }
    let command_group = u16::from_be_bytes([payload[0], payload[1]]);
    let command_word = u16::from_be_bytes([payload[2], payload[3]]);
    Ok((command_group, (command_word & 0x8000) != 0, command_word & 0x7fff, &payload[4..]))
}

fn decode_tnc_data_fragment(payload: &[u8]) -> Result<TncDataFragment<'_>, Vrn76KissBleError> {
    let Some((&header, rest)) = payload.split_first() else {
        return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
    };
    let is_final_fragment = (header & 0x80) != 0;
    let has_channel_id = (header & 0x40) != 0;
    let fragment_id = header & 0x3f;
    let (payload, channel_id) = if has_channel_id {
        let Some((&channel_id, payload)) = rest.split_last() else {
            return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
        };
        (payload, Some(channel_id))
    } else {
        (rest, None)
    };
    Ok(TncDataFragment { fragment_id, is_final: is_final_fragment, channel_id, payload })
}
