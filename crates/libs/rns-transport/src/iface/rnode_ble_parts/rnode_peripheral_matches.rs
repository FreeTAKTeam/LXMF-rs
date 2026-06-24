pub struct RnodeBleKissRuntime<B> {
    backend: B,
    session: RnodeBleKissSession,
    connected: bool,
}

impl<B> RnodeBleKissRuntime<B>
where
    B: RnodeBleBackend,
{
    #[must_use]
    pub fn new(backend: B, config: RnodeBleKissConfig) -> Self {
        Self { backend, session: RnodeBleKissSession::new(config), connected: false }
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
    pub fn status(&self) -> RnodeBleKissStatus {
        self.session.status_with_connection(self.connected)
    }

    #[must_use]
    pub fn negotiated_mtu(&self) -> Option<u16> {
        self.backend.negotiated_mtu()
    }

    pub async fn startup(&mut self) -> Result<(), RnodeBleKissError> {
        self.connected = false;
        self.backend
            .connect()
            .await
            .map_err(|message| RnodeBleKissError::Backend { operation: "connect", message })?;
        if let Some(mtu) = self.backend.negotiated_mtu() {
            let att_payload = (mtu as usize).saturating_sub(3);
            self.session.config.max_write_len = att_payload.min(self.session.config.mtu);
        }
        self.backend.subscribe_notifications().await.map_err(|message| {
            RnodeBleKissError::Backend { operation: "subscribe_notifications", message }
        })?;
        #[cfg(feature = "rnode-ble")]
        self.drain_startup_notifications().await?;
        let writes = self.session.startup_frames();
        self.write_all(writes, "startup_write").await?;
        self.connected = true;
        Ok(())
    }

    #[cfg(feature = "rnode-ble")]
    pub async fn send_deferred_frames(&mut self) -> Result<(), RnodeBleKissError> {
        let writes = self.session.deferred_frames();
        self.write_all(writes, "deferred_frames_write").await
    }

    pub async fn send_packet(&mut self, payload: &[u8]) -> Result<(), RnodeBleKissError> {
        if payload.len() > self.session.mtu() {
            return Err(RnodeBleKissError::PacketTooLarge {
                limit: self.session.mtu(),
                actual: payload.len(),
            });
        }
        let writes = self.session.enqueue_packet(payload);
        self.write_all(writes, "write_packet").await
    }

    pub async fn send_id_beacon(&mut self) -> Result<(), RnodeBleKissError> {
        let writes = self.session.enqueue_id_beacon();
        self.write_all(writes, "write_id_beacon").await
    }

    pub async fn shutdown(&mut self) -> Result<(), RnodeBleKissError> {
        let writes = self.session.shutdown_frames();
        self.write_all(writes, "shutdown_write").await
    }

    pub async fn poll_notification(&mut self) -> Result<Vec<Vec<u8>>, RnodeBleKissError> {
        Ok(self.poll_notification_events().await?.packets)
    }

    pub async fn poll_notification_events(
        &mut self,
    ) -> Result<RnodeBleNotification, RnodeBleKissError> {
        let Some(payload) = self.backend.next_notification().await.map_err(|message| {
            self.connected = false;
            RnodeBleKissError::Backend { operation: "next_notification", message }
        })?
        else {
            return Ok(RnodeBleNotification::default());
        };
        {
            let hex: String = payload
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ");
            log::trace!("RNode BLE raw notification {} bytes: [{}]", payload.len(), hex);
        }
        let notification = self.session.accept_notification_events(&payload)?;
        let writes = self.session.take_pending_writes();
        self.write_all(writes, "write_pending").await?;
        Ok(notification)
    }

    async fn write_all(
        &mut self,
        writes: Vec<RnodeBleWrite>,
        operation: &'static str,
    ) -> Result<(), RnodeBleKissError> {
        for write in writes {
            self.backend.write(write).await.map_err(|message| {
                self.connected = false;
                RnodeBleKissError::Backend { operation, message }
            })?;
        }
        Ok(())
    }
}

#[cfg(feature = "rnode-ble")]
#[derive(Debug, Clone)]
pub struct NativeRnodeBleKissInterface {
    label: String,
    settings: NativeRnodeBleSettings,
    config: RnodeBleKissConfig,
    rnode_config: Option<LoraConfig>,
    startup_response_timeout: Duration,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    detection_fallback_timeout: Option<Duration>,
}

#[cfg(feature = "rnode-ble")]
impl NativeRnodeBleKissInterface {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        settings: NativeRnodeBleSettings,
        config: RnodeBleKissConfig,
    ) -> Self {
        Self {
            label: label.into(),
            settings,
            config,
            rnode_config: None,
            // TODO: startup_response_timeout should not exist. The device should send an
            //       explicit "ready" notification after completing startup, removing the
            //       need for a client-side deadline entirely. Consider raising a firmware
            //       feature request with markqvist (https://github.com/markqvist/RNode_Firmware)
            //       to add a CMD_READY or equivalent handshake frame.
            startup_response_timeout: Duration::from_millis(5_000), // was 1_500; matches Python's ble_detect_timeout
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            detection_fallback_timeout: None,
        }
    }

    #[must_use]
    pub fn with_rnode_validation(
        mut self,
        rnode_config: LoraConfig,
        startup_response_timeout: Duration,
    ) -> Self {
        self.rnode_config = Some(rnode_config);
        self.startup_response_timeout = startup_response_timeout;
        self
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

    /// If CMD_DETECT response has not arrived within `timeout` of session establishment,
    /// send the deferred radio-config frames unconditionally. Useful for firmware that
    /// does not respond to the first probe on a fresh BLE connection.
    #[must_use]
    pub fn with_detection_fallback_timeout(mut self, timeout: Duration) -> Self {
        self.detection_fallback_timeout = Some(timeout);
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (
            label,
            settings,
            config,
            rnode_config,
            startup_response_timeout,
            reconnect_backoff,
            max_reconnect_backoff,
            detection_fallback_timeout,
        ) = {
            let guard = context.inner.lock().expect("RNode BLE interface mutex poisoned");
            (
                guard.label.clone(),
                guard.settings.clone(),
                guard.config.clone(),
                guard.rnode_config,
                guard.startup_response_timeout,
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.detection_fallback_timeout,
            )
        };
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let backend = NativeRnodeBleBackend::new(settings.clone());
            let mut runtime = RnodeBleKissRuntime::new(backend, config.clone());
            if let Err(err) = runtime.startup().await {
                log::warn!(
                    "RNode KISS-over-BLE session setup failed iface={} addr={} err={:?}",
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
            active_backoff = reconnect_backoff;
            log::info!(
                "RNode KISS-over-BLE session established iface={} addr={} peripheral_id={}",
                label,
                iface_address,
                settings.peripheral_id
            );
            // RNODE_LXMF_MIN_ATT_MTU = 173 (170 notification payload bytes + 3 ATT header)
            match runtime.negotiated_mtu() {
                Some(mtu) if mtu < 173 => log::warn!(
                    "RNode BLE negotiated ATT MTU {} < 173 minimum for LXMF; \
                     expect incomplete notification payloads iface={}",
                    mtu,
                    label
                ),
                Some(mtu) => log::info!("RNode BLE negotiated ATT MTU {} iface={}", mtu, label),
                None => log::debug!(
                    "RNode BLE negotiated ATT MTU unknown (macOS or non-native backend) iface={}",
                    label
                ),
            }

            let mut tx_buffer = vec![0_u8; config.mtu];
            let mut reconnect_needed = false;
            let mut command_monitor = rnode_config
                .map(|config| RnodeBleCommandMonitor::new(config, startup_response_timeout));
            let mut radio_config_sent = command_monitor.is_none();
            log::info!(
                "RNode BLE session ready: command_monitor={} radio_config_sent={} iface={}",
                command_monitor.is_some(),
                radio_config_sent,
                label
            );
            let mut detection_fallback_deadline: Option<TokioInstant> =
                if command_monitor.is_some() {
                    detection_fallback_timeout.map(|t| TokioInstant::now() + t)
                } else {
                    None
                };
            let mut first_tx_at: Option<TokioInstant> = None;
            while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
                if !radio_config_sent {
                    if let Some(deadline) = detection_fallback_deadline {
                        if TokioInstant::now() >= deadline {
                            detection_fallback_deadline = None;
                            log::warn!(
                                "RNode BLE detection fallback: CMD_DETECT not received within \
                                 timeout, sending deferred frames anyway iface={}",
                                label
                            );
                            radio_config_sent = true;
                            if let Err(err) = runtime.send_deferred_frames().await {
                                log::warn!(
                                    "RNode BLE radio config write (fallback) failed iface={} err={:?}",
                                    label,
                                    err
                                );
                                reconnect_needed = true;
                            } else if let Some(mon) = command_monitor.as_mut() {
                                mon.accept_degraded_startup();
                            }
                        }
                    }
                }
                if reconnect_needed {
                    break;
                }
                if radio_config_sent {
                    while let Ok(message) = tx_channel.try_recv() {
                        let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                        if message.packet.serialize(&mut output).is_err() {
                            log::warn!("RNode BLE packet serialize failed iface={}", label);
                            continue;
                        }
                        if let Err(err) = runtime.send_packet(output.as_slice()).await {
                            log::warn!(
                                "RNode BLE packet write failed iface={} err={:?}",
                                label,
                                err
                            );
                            reconnect_needed = true;
                            break;
                        }
                        if first_tx_at.is_none() {
                            first_tx_at = Some(TokioInstant::now());
                        }
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
                                "RNode BLE station ID write failed iface={} err={:?}",
                                label,
                                err
                            );
                            reconnect_needed = true;
                            break;
                        }
                        first_tx_at = None;
                    }
                }

                match timeout(Duration::from_millis(100), runtime.poll_notification_events()).await
                {
                    Ok(Ok(notification)) => {
                        if !notification.packets.is_empty() || !notification.commands.is_empty() {
                            log::debug!(
                                "RNode BLE notification: {} data packets, {} commands iface={}",
                                notification.packets.len(),
                                notification.commands.len(),
                                label
                            );
                        }
                        if let Some(monitor) = command_monitor.as_mut() {
                            if let Err(err) = monitor.accept_notification(&notification) {
                                log::warn!(
                                    "RNode BLE command response validation failed iface={} err={}",
                                    label,
                                    err
                                );
                                reconnect_needed = true;
                                break;
                            }
                            if !radio_config_sent && monitor.is_detected() {
                                log::info!(
                                    "RNode BLE detected (CMD_DETECT response received), \
                                     sending radio config iface={}",
                                    label
                                );
                                radio_config_sent = true;
                                if let Err(err) = runtime.send_deferred_frames().await {
                                    log::warn!(
                                        "RNode BLE radio config write failed iface={} err={:?}",
                                        label,
                                        err
                                    );
                                    reconnect_needed = true;
                                } else {
                                    monitor.reset_startup_deadline(startup_response_timeout);
                                }
                            }
                        }
                        if reconnect_needed {
                            break;
                        }
                        for payload in notification.packets {
                            match Packet::deserialize(&mut InputBuffer::new(&payload)) {
                                Ok(packet) => {
                                    log::debug!(
                                        "RNode BLE rx packet len={} iface={}",
                                        payload.len(),
                                        label
                                    );
                                    let _ = rx_channel
                                        .send(RxMessage {
                                            address: iface_address,
                                            packet,
                                            source: IfaceSource::None,
                                        })
                                        .await;
                                }
                                Err(err) => {
                                    let hex: String = payload
                                        .iter()
                                        .map(|b| format!("{:02x}", b))
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    log::warn!(
                                        "RNode BLE rx packet deserialize failed len={} err={:?} bytes=[{}] iface={}",
                                        payload.len(),
                                        err,
                                        hex,
                                        label
                                    );
                                }
                            }
                        }
                    }
                    Err(_) => {}
                    Ok(Err(err)) => {
                        log::warn!("RNode BLE packet read failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                }
                if let Some(monitor) = command_monitor.as_mut() {
                    if let Err(err) = monitor.validate_startup_deadline() {
                        log::warn!(
                            "RNode BLE startup response validation failed iface={} err={}",
                            label,
                            err
                        );
                        reconnect_needed = true;
                        break;
                    }
                }
            }

            let _ = runtime.shutdown().await;
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

#[cfg(feature = "rnode-ble")]
impl Interface for NativeRnodeBleKissInterface {
    fn mtu() -> usize {
        508
    }

    fn configured_mtu(&self) -> usize {
        self.config.mtu
    }
}

#[derive(Debug, Clone)]
pub struct RnodeBleCommandMonitor {
    lora: LoraInterface,
    startup_deadline: Option<Instant>,
}
