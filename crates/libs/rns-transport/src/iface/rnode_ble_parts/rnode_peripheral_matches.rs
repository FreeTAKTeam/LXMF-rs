pub struct RnodeBleKissRuntime<B> {
    backend: B,
    session: RnodeBleKissSession,
    connected: bool,
    io_stats: RnodeBleKissIoStats,
}

impl<B> RnodeBleKissRuntime<B>
where
    B: RnodeBleBackend,
{
    #[must_use]
    pub fn new(backend: B, config: RnodeBleKissConfig) -> Self {
        Self {
            backend,
            session: RnodeBleKissSession::new(config),
            connected: false,
            io_stats: RnodeBleKissIoStats::default(),
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
    pub fn status(&self) -> RnodeBleKissStatus {
        self.session.status_with_connection(self.connected)
    }

    #[must_use]
    pub fn io_stats(&self) -> RnodeBleKissIoStats {
        self.io_stats
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
            self.session.config.max_write_len = self
                .session
                .config
                .max_write_len
                .min(att_payload)
                .min(self.session.config.mtu);
        }
        self.backend.subscribe_notifications().await.map_err(|message| {
            RnodeBleKissError::Backend { operation: "subscribe_notifications", message }
        })?;
        if self.backend.drains_stale_startup_notifications() {
            self.drain_startup_notifications().await?;
        }
        let writes = self.session.startup_frames();
        self.write_all(writes, "startup_write").await?;
        self.connected = true;
        Ok(())
    }

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

    pub async fn send_management_frame(
        &mut self,
        frame: Vec<u8>,
    ) -> Result<(), RnodeBleKissError> {
        let writes = self.session.management_frame_writes(frame);
        self.write_all(writes, "write_management_frame").await
    }

    pub async fn poll_queue_admission(&mut self) -> Result<(), RnodeBleKissError> {
        let writes = self.session.queue_admission_probe_if_due();
        self.write_all(writes, "write_queue_admission_probe").await
    }

    pub async fn shutdown(&mut self) -> Result<(), RnodeBleKissError> {
        self.shutdown_with_prefix_frames(Vec::new()).await
    }

    pub async fn shutdown_with_prefix_frames(
        &mut self,
        prefix_frames: Vec<Vec<u8>>,
    ) -> Result<(), RnodeBleKissError> {
        let writes = self.session.shutdown_frames_with_prefix(prefix_frames);
        let write_result = self.write_all(writes, "shutdown_write").await;
        let close_result = self.backend.close().await.map_err(|message| RnodeBleKissError::Backend {
            operation: "close",
            message,
        });
        self.connected = false;
        write_result.and(close_result)
    }

    pub async fn close(&mut self) -> Result<(), RnodeBleKissError> {
        let result = self.backend.close().await.map_err(|message| RnodeBleKissError::Backend {
            operation: "close",
            message,
        });
        self.connected = false;
        result
    }

    pub async fn poll_notification(&mut self) -> Result<Vec<Vec<u8>>, RnodeBleKissError> {
        Ok(self.poll_notification_events().await?.packets)
    }

    pub async fn poll_notification_events(
        &mut self,
    ) -> Result<RnodeBleNotification, RnodeBleKissError> {
        Ok(self.poll_optional_notification_events().await?.unwrap_or_default())
    }

    pub(crate) async fn poll_optional_notification_events(
        &mut self,
    ) -> Result<Option<RnodeBleNotification>, RnodeBleKissError> {
        let Some(payload) = self.backend.next_notification().await.map_err(|message| {
            self.connected = false;
            RnodeBleKissError::Backend { operation: "next_notification", message }
        })?
        else {
            return Ok(None);
        };
        self.io_stats.read_chunks = self.io_stats.read_chunks.saturating_add(1);
        self.io_stats.read_bytes = self
            .io_stats
            .read_bytes
            .saturating_add(u64::try_from(payload.len()).unwrap_or(u64::MAX));
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
        Ok(Some(notification))
    }

    async fn write_all(
        &mut self,
        writes: Vec<RnodeBleWrite>,
        operation: &'static str,
    ) -> Result<(), RnodeBleKissError> {
        for write in writes {
            let payload_len = write.payload.len();
            self.backend.write(write).await.map_err(|message| {
                self.connected = false;
                RnodeBleKissError::Backend { operation, message }
            })?;
            self.io_stats.write_chunks = self.io_stats.write_chunks.saturating_add(1);
            self.io_stats.write_bytes = self
                .io_stats
                .write_bytes
                .saturating_add(u64::try_from(payload_len).unwrap_or(u64::MAX));
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
    rnode_status: Option<Arc<Mutex<serde_json::Value>>>,
    startup_response_timeout: Duration,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    detection_fallback_timeout: Option<Duration>,
    management_frame_tx: RnodeBleManagementFrameSender,
    management_frame_rx: RnodeBleManagementFrameReceiver,
}

#[cfg(feature = "rnode-ble")]
impl NativeRnodeBleKissInterface {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        settings: NativeRnodeBleSettings,
        config: RnodeBleKissConfig,
    ) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_ble_management_channel();
        Self {
            label: label.into(),
            settings,
            config,
            rnode_config: None,
            rnode_status: None,
            // TODO: startup_response_timeout should not exist. The device should send an
            //       explicit "ready" notification after completing startup, removing the
            //       need for a client-side deadline entirely. Consider raising a firmware
            //       feature request with markqvist (https://github.com/markqvist/RNode_Firmware)
            //       to add a CMD_READY or equivalent handshake frame.
            startup_response_timeout: Duration::from_millis(5_000), // was 1_500; matches Python's ble_detect_timeout
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            detection_fallback_timeout: None,
            management_frame_tx,
            management_frame_rx,
        }
    }

    #[must_use]
    pub fn with_rnode_validation(
        mut self,
        rnode_config: LoraConfig,
        startup_response_timeout: Duration,
    ) -> Self {
        let endpoint = format!("ble://{}", self.settings.peripheral_id);
        self.rnode_status = Some(Arc::new(Mutex::new(rnode_ble_initial_runtime_status_json(
            rnode_config,
            endpoint.as_str(),
        ))));
        self.rnode_config = Some(rnode_config);
        self.startup_response_timeout = startup_response_timeout;
        self
    }

    #[must_use]
    pub fn runtime_status_handle(&self) -> Option<RnodeBleRuntimeStatusHandle> {
        self.rnode_status.as_ref().map(|inner| RnodeBleRuntimeStatusHandle::new(inner.clone()))
    }

    #[must_use]
    pub fn rnode_management_handle(&self) -> RnodeBleManagementHandle {
        RnodeBleManagementHandle { tx: self.management_frame_tx.clone() }
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
            rnode_status,
            startup_response_timeout,
            reconnect_backoff,
            max_reconnect_backoff,
            detection_fallback_timeout,
            management_frame_rx,
        ) = {
            let guard = context.inner.lock().expect("RNode BLE interface mutex poisoned");
            (
                guard.label.clone(),
                guard.settings.clone(),
                guard.config.clone(),
                guard.rnode_config,
                guard.rnode_status.clone(),
                guard.startup_response_timeout,
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.detection_fallback_timeout,
                guard.management_frame_rx.clone(),
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
                if let Err(cleanup_error) = backend.cleanup().await {
                    log::warn!(
                        "RNode BLE cleanup failed after setup error iface={} error={cleanup_error:?}",
                        label
                    );
                }
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
            match runtime.negotiated_mtu() {
                Some(mtu) => log::info!("RNode BLE negotiated ATT MTU {} iface={}", mtu, label),
                None => log::debug!(
                    "RNode BLE negotiated ATT MTU unknown (macOS or non-native backend) iface={}",
                    label
                ),
            }

            let packet_mtu = config.mtu;
            let mut reconnect_needed = false;
            let mut command_monitor = rnode_config
                .map(|config| RnodeBleCommandMonitor::new(config, startup_response_timeout));
            if let (Some(monitor), Some(status)) =
                (command_monitor.as_ref(), rnode_status.as_ref())
            {
                *status.lock().expect("RNode BLE status mutex poisoned") =
                    monitor.runtime_status_json(format!("ble://{}", settings.peripheral_id).as_str());
            }
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
                                mon.reset_startup_deadline(startup_response_timeout);
                            }
                        }
                    }
                }
                if reconnect_needed {
                    break;
                }
                if radio_config_sent {
                    let management_frames = {
                        let mut rx = management_frame_rx.lock().await;
                        let mut frames = Vec::new();
                        while let Ok(frame) = rx.try_recv() {
                            frames.push(frame);
                        }
                        frames
                    };
                    for frame in management_frames {
                        if let Err(err) = runtime.send_management_frame(frame).await {
                            log::warn!(
                                "RNode BLE management frame write failed iface={} err={:?}",
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

                if rnode_ble_payload_writes_enabled(radio_config_sent, command_monitor.as_ref()) {
                    while let Ok(message) = tx_channel.try_recv() {
                        let raw = match message.packet.to_bytes() {
                            Ok(raw) => raw,
                            Err(err) => {
                                log::warn!(
                                    "RNode BLE packet serialize failed iface={} packet_type={:?} \
                                     context={:?} dst={} data_len={} mtu={} err={:?}",
                                    label,
                                    message.packet.header.packet_type,
                                    message.packet.context,
                                    message.packet.destination,
                                    message.packet.data.len(),
                                    packet_mtu,
                                    err
                                );
                                continue;
                            }
                        };
                        if raw.len() > packet_mtu {
                            log::warn!(
                                "RNode BLE packet exceeds configured MTU iface={} packet_type={:?} \
                                 context={:?} dst={} data_len={} wire_len={} mtu={}",
                                label,
                                message.packet.header.packet_type,
                                message.packet.context,
                                message.packet.destination,
                                message.packet.data.len(),
                                raw.len(),
                                packet_mtu
                            );
                            continue;
                        }
                        if let Err(err) = runtime.send_packet(&raw).await {
                            log::warn!(
                                "RNode BLE packet write failed iface={} packet_type={:?} \
                                 context={:?} dst={} wire_len={} mtu={} err={:?}",
                                label,
                                message.packet.header.packet_type,
                                message.packet.context,
                                message.packet.destination,
                                raw.len(),
                                packet_mtu,
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

                if let Err(err) = runtime.poll_queue_admission().await {
                    log::warn!(
                        "RNode BLE queue-admission probe failed iface={} err={:?}",
                        label,
                        err
                    );
                    reconnect_needed = true;
                    break;
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
                            if let Some(status) = rnode_status.as_ref() {
                                *status.lock().expect("RNode BLE status mutex poisoned") = monitor
                                    .runtime_status_json(
                                        format!("ble://{}", settings.peripheral_id).as_str(),
                                    );
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
                                    if rx_channel
                                        .send(RxMessage {
                                            address: iface_address,
                                            packet,
                                            source: IfaceSource::None,
                                        })
                                        .await
                                        .is_err()
                                    {
                                        log::warn!(
                                            "RNode BLE transport receive queue closed iface={label}"
                                        );
                                        iface_stop.cancel();
                                        break;
                                    }
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
                    if let Some(status) = rnode_status.as_ref() {
                        *status.lock().expect("RNode BLE status mutex poisoned") = monitor
                            .runtime_status_json(format!("ble://{}", settings.peripheral_id).as_str());
                    }
                }
            }

            let shutdown_prefix_frames = command_monitor
                .as_ref()
                .and_then(|monitor| monitor.external_framebuffer_frame(false))
                .into_iter()
                .collect::<Vec<_>>();
            if let Err(error) = runtime.shutdown_with_prefix_frames(shutdown_prefix_frames).await {
                log::warn!("RNode BLE shutdown failed iface={} error={error:?}", label);
            }
            let mut backend = runtime.into_backend();
            if let Err(error) = backend.cleanup().await {
                log::warn!("RNode BLE cleanup failed iface={} error={error:?}", label);
            }
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
    startup_validated: bool,
    startup_payload_writes_enabled: bool,
    startup_compatibility_warning: Option<String>,
}
