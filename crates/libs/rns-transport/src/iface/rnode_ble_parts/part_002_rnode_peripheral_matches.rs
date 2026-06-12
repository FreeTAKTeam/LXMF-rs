#[cfg(feature = "rnode-ble")]
async fn rnode_peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
) -> Result<bool, String> {
    if native_rnode_identifier_matches(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        if native_rnode_identifier_matches(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if native_rnode_identifier_matches(configured_id, &local_name) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(feature = "rnode-ble")]
fn parse_rnode_uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("RNode BLE UUID constants must be valid")
}

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

    pub async fn startup(&mut self) -> Result<(), RnodeBleKissError> {
        self.connected = false;
        self.backend
            .connect()
            .await
            .map_err(|message| RnodeBleKissError::Backend { operation: "connect", message })?;
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
    async fn drain_startup_notifications(&mut self) -> Result<(), RnodeBleKissError> {
        let deadline = TokioInstant::now() + RNODE_BLE_STARTUP_STABILIZATION_TIMEOUT;
        let mut drained = 0_usize;
        loop {
            let now = TokioInstant::now();
            if now >= deadline {
                break;
            }
            let quiet_timeout = deadline
                .saturating_duration_since(now)
                .min(RNODE_BLE_STARTUP_NOTIFICATION_QUIET_TIMEOUT);
            match timeout(quiet_timeout, self.backend.next_notification()).await {
                Ok(Ok(Some(_))) => {
                    drained += 1;
                }
                Ok(Ok(None)) | Err(_) => break,
                Ok(Err(message)) => {
                    self.connected = false;
                    return Err(RnodeBleKissError::Backend {
                        operation: "drain_startup_notifications",
                        message,
                    });
                }
            }
        }
        if drained > 0 {
            log::debug!("drained {drained} stale RNode BLE startup notifications");
        }
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

            let mut tx_buffer = vec![0_u8; config.mtu];
            let mut reconnect_needed = false;
            let mut command_monitor = rnode_config
                .map(|config| RnodeBleCommandMonitor::new(config, startup_response_timeout));
            let mut radio_config_sent = command_monitor.is_none();
            let mut first_tx_at: Option<TokioInstant> = None;
            while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
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
                            if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&payload))
                            {
                                let _ = rx_channel
                                    .send(RxMessage {
                                        address: iface_address,
                                        packet,
                                        source: IfaceSource::None,
                                    })
                                    .await;
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

#[cfg(feature = "rnode-ble")]
fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

#[derive(Debug, Clone)]
pub struct RnodeBleCommandMonitor {
    lora: LoraInterface,
    startup_deadline: Option<Instant>,
}
