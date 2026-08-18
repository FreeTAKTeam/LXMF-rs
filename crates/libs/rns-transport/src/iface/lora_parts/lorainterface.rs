type RNodeManagementFrameSender = tokio::sync::mpsc::Sender<Vec<u8>>;
type RNodeManagementFrameReceiver =
    Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>;
type RNodeManagementStatus = Arc<std::sync::Mutex<LoraRNodeManagementStatus>>;

#[derive(Debug, Clone, Default)]
struct LoraRNodeManagementStatus {
    accepted_total: u64,
    failed_total: u64,
    next_operation_seq: u64,
    last_operation: Option<LoraRNodeManagementOperation>,
    last_management_error: Option<String>,
}

#[derive(Debug, Clone)]
struct LoraRNodeManagementOperation {
    operation_id: String,
    command: String,
    state: String,
}

impl LoraRNodeManagementStatus {
    fn record_queued(&mut self, command: &str) {
        let operation_id = self.next_operation_id();
        self.accepted_total = self.accepted_total.saturating_add(1);
        self.last_operation = Some(LoraRNodeManagementOperation {
            operation_id,
            command: command.to_owned(),
            state: "queued".to_owned(),
        });
        self.last_management_error = None;
    }

    fn record_failed(&mut self, command: &str, error: String) {
        let operation_id = self.next_operation_id();
        self.failed_total = self.failed_total.saturating_add(1);
        self.last_operation = Some(LoraRNodeManagementOperation {
            operation_id,
            command: command.to_owned(),
            state: "failed".to_owned(),
        });
        self.last_management_error = Some(error);
    }

    fn next_operation_id(&mut self) -> String {
        self.next_operation_seq = self.next_operation_seq.saturating_add(1);
        format!("rnode-management-{}", self.next_operation_seq)
    }

    fn to_json(&self) -> serde_json::Value {
        let last_operation = self.last_operation.as_ref().map(|operation| {
            serde_json::json!({
                "operation_id": operation.operation_id,
                "command": operation.command,
                "state": operation.state,
            })
        });
        serde_json::json!({
            "accepted_total": self.accepted_total,
            "failed_total": self.failed_total,
            "last_operation": last_operation,
            "last_management_error": self.last_management_error.as_deref(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LoraInterface {
    endpoint: LoraEndpoint,
    config: LoraConfig,
    probe_status: RNodeProbeStatus,
    radio_status: RNodeRadioStatus,
    hardware_errors: Vec<RNodeHardwareError>,
    last_command_error: Option<String>,
    online: bool,
    radio_state_response_seen: bool,
    flow_control: bool,
    id_beacon: Option<KissIdBeaconConfig>,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    startup_response_timeout: Duration,
    management_frame_tx: RNodeManagementFrameSender,
    management_frame_rx: RNodeManagementFrameReceiver,
    management_status: RNodeManagementStatus,
}

impl LoraInterface {
    #[must_use]
    pub fn new<T: Into<String>>(device: T, baud_rate: u32, config: LoraConfig) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_management_channel();
        Self {
            endpoint: LoraEndpoint::Serial { device: device.into(), baud_rate },
            config,
            probe_status: RNodeProbeStatus::default(),
            radio_status: RNodeRadioStatus::default(),
            hardware_errors: Vec::new(),
            last_command_error: None,
            online: false,
            radio_state_response_seen: false,
            flow_control: false,
            id_beacon: None,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            startup_response_timeout: R_NODE_STARTUP_RESPONSE_TIMEOUT,
            management_frame_tx,
            management_frame_rx,
            management_status: Arc::new(std::sync::Mutex::new(LoraRNodeManagementStatus::default())),
        }
    }

    #[must_use]
    pub fn new_tcp<T: Into<String>>(addr: T, config: LoraConfig) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_management_channel();
        Self {
            endpoint: LoraEndpoint::Tcp { addr: addr.into() },
            config,
            probe_status: RNodeProbeStatus::default(),
            radio_status: RNodeRadioStatus::default(),
            hardware_errors: Vec::new(),
            last_command_error: None,
            online: false,
            radio_state_response_seen: false,
            flow_control: false,
            id_beacon: None,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            startup_response_timeout: R_NODE_STARTUP_RESPONSE_TIMEOUT,
            management_frame_tx,
            management_frame_rx,
            management_status: Arc::new(std::sync::Mutex::new(LoraRNodeManagementStatus::default())),
        }
    }

    #[must_use]
    pub fn bearer(&self) -> LoraBearer {
        self.endpoint.bearer()
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        self.endpoint.label()
    }

    #[must_use]
    pub fn baud_rate(&self) -> Option<u32> {
        self.endpoint.baud_rate()
    }

    #[must_use]
    pub fn activity_probe(&self) -> Option<KissActivityProbeConfig> {
        (self.endpoint.bearer() == LoraBearer::Tcp).then(rnode_tcp_activity_probe)
    }

    #[must_use]
    pub fn config(&self) -> LoraConfig {
        self.config
    }

    #[must_use]
    pub fn probe_status(&self) -> RNodeProbeStatus {
        self.probe_status
    }

    #[must_use]
    pub fn radio_status(&self) -> RNodeRadioStatus {
        self.radio_status.clone()
    }

    #[must_use]
    pub fn hardware_errors(&self) -> &[RNodeHardwareError] {
        &self.hardware_errors
    }

    #[must_use]
    pub fn last_command_error(&self) -> Option<&str> {
        self.last_command_error.as_deref()
    }

    #[must_use]
    pub fn online(&self) -> bool {
        self.online
    }

    #[must_use]
    pub fn flow_control(&self) -> bool {
        self.flow_control
    }

    #[must_use]
    pub fn startup_response_timeout(&self) -> Duration {
        self.startup_response_timeout
    }

    #[must_use]
    pub fn rnode_management_handle(&self) -> LoraRNodeManagementHandle {
        LoraRNodeManagementHandle {
            tx: self.management_frame_tx.clone(),
            status: self.management_status.clone(),
        }
    }

    #[must_use]
    pub fn with_flow_control(mut self, flow_control: bool) -> Self {
        self.flow_control = flow_control;
        self
    }

    #[must_use]
    pub fn with_id_beacon(mut self, id_beacon: Option<KissIdBeaconConfig>) -> Self {
        self.id_beacon = id_beacon;
        self
    }

    pub fn record_probe_command(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        self.probe_status.accept_command(command, payload)
    }

    pub fn begin_startup_response_collection(&mut self) {
        self.probe_status = RNodeProbeStatus::default();
        self.radio_status = RNodeRadioStatus::default();
        self.hardware_errors.clear();
        self.last_command_error = None;
        self.online = false;
        self.radio_state_response_seen = false;
    }

    pub fn record_command_response(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        if command == CMD_RADIO_STATE {
            self.radio_state_response_seen = true;
        }
        if self.probe_status.accept_command(command, payload)? {
            return Ok(true);
        }
        if command == CMD_RESET {
            return match self.probe_status.accept_reset_response(payload, self.online) {
                Ok(accepted) => Ok(accepted),
                Err(err) => {
                    self.last_command_error = Some(err.clone());
                    Err(err)
                }
            };
        }
        if command == CMD_ERROR {
            let code = single_byte_payload(payload, "hardware error")?;
            let error = RNodeHardwareError::from_code(code);
            if error.fatal {
                self.last_command_error = Some(error.description.to_string());
                return Err(error.description.to_string());
            }
            self.hardware_errors.push(error);
            return Ok(true);
        }
        let accepted = self.radio_status.accept_command(command, payload)?;
        if accepted && command == CMD_RADIO_STATE {
            self.online = self.radio_status.radio_state == Some(RADIO_STATE_ON);
        }
        Ok(accepted)
    }

    pub fn record_inbound_data_frame(&mut self) {
        self.radio_status.rssi_dbm = None;
        self.radio_status.snr_db = None;
    }

    pub fn is_detected(&self) -> bool {
        self.probe_status.detected
    }

    pub fn validate_probe_status(&self) -> Result<(), String> {
        self.probe_status.validate_startup_probe()
    }

    pub fn validate_radio_status(&self) -> Result<(), String> {
        self.radio_status.validate_config(self.config, RADIO_STATE_ON)
    }

    pub fn validate_startup_responses(&self) -> Result<(), String> {
        if let Some(err) = self.last_command_error() {
            return Err(err.to_string());
        }
        self.validate_probe_status()?;
        self.validate_radio_status()
    }

    /// Accept older RNode firmware that applies the requested radio state but
    /// does not echo `CMD_RADIO_STATE` during startup.
    ///
    /// This remains fail-closed for every reported radio parameter, probe
    /// requirement, hardware error, and explicit radio-state mismatch. It is
    /// only usable when the radio-state response is absent.
    pub fn accept_missing_radio_state_compatibility(&mut self) -> Result<bool, String> {
        if self.radio_status.radio_state.is_some() || self.radio_state_response_seen {
            return Ok(false);
        }
        if let Some(err) = self.last_command_error() {
            return Err(err.to_string());
        }
        self.validate_probe_status()?;
        let mut compatible_status = self.radio_status.clone();
        compatible_status.radio_state = Some(RADIO_STATE_ON);
        compatible_status.validate_config(self.config, RADIO_STATE_ON)?;
        self.online = true;
        Ok(true)
    }

    pub fn reported_bitrate_bps(&self) -> Option<f64> {
        self.radio_status.reported_bitrate_bps()
    }

    #[must_use]
    pub fn runtime_status_json(&self) -> serde_json::Value {
        let configured = serde_json::json!({
            "frequency_hz": self.config.frequency_hz,
            "bandwidth_hz": self.config.bandwidth_hz,
            "spreading_factor": self.config.spreading_factor,
            "coding_rate": self.config.coding_rate,
            "tx_power_dbm": self.config.tx_power_dbm,
            "max_payload_bytes": self.config.max_payload_bytes,
        });
        let management_status =
            self.management_status.lock().expect("lora management status mutex poisoned").to_json();
        let management = serde_json::json!({
            "supported": true,
            "safe_commands": ["radio_state_query", "blink"],
            "guarded_persistent_commands": true,
            "guarded_destructive_commands": true,
            "queue": {
                "available_capacity": self.management_frame_tx.capacity(),
                "max_capacity": self.management_frame_tx.max_capacity(),
                "pending_depth": self
                    .management_frame_tx
                    .max_capacity()
                    .saturating_sub(self.management_frame_tx.capacity()),
                "closed": self.management_frame_tx.is_closed(),
            },
            "operations": management_status,
        });
        serde_json::json!({
            "endpoint": self.endpoint(),
            "bearer": match self.bearer() {
                LoraBearer::Serial => "serial",
                LoraBearer::Tcp => "tcp",
            },
            "baud_rate": self.baud_rate(),
            "configured": configured,
            "probe_status": self.probe_status.to_json(),
            "radio_status": self.radio_status.to_json(),
            "hardware_errors": self
                .hardware_errors
                .iter()
                .copied()
                .map(RNodeHardwareError::to_json)
                .collect::<Vec<_>>(),
            "last_command_error": self.last_command_error.as_deref(),
            "online": self.online,
            "flow_control": self.flow_control,
            "reported_bitrate_bps": self.reported_bitrate_bps(),
            "management": management,
        })
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

    #[must_use]
    pub fn with_startup_response_timeout(mut self, startup_response_timeout: Duration) -> Self {
        self.startup_response_timeout = startup_response_timeout;
        self
    }

    pub fn preflight_open(&self) -> Result<(), String> {
        self.config.validate()?;
        match &self.endpoint {
            LoraEndpoint::Serial { device, baud_rate } => {
                tokio_serial::new(device.clone(), *baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(FlowControl::None)
                    .open_native_async()
                    .map(|_| ())
                    .map_err(|err| {
                        format!(
                            "lora preflight open failed device={} baud_rate={} err={}",
                            device, baud_rate, err
                        )
                    })
            }
            LoraEndpoint::Tcp { addr } => preflight_tcp_connect(addr),
        }
    }

    pub async fn spawn(context: InterfaceContext<LoraInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (
            endpoint,
            config,
            flow_control,
            id_beacon,
            reconnect_backoff,
            max_reconnect_backoff,
            startup_response_timeout,
            management_frame_rx,
        ) = {
            let guard = context.inner.lock().expect("lora interface mutex poisoned");
            (
                guard.endpoint.clone(),
                guard.config,
                guard.flow_control,
                guard.id_beacon.clone(),
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.startup_response_timeout,
                guard.management_frame_rx.clone(),
            )
        };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            match &endpoint {
                LoraEndpoint::Serial { device, baud_rate } => {
                    let port = match tokio_serial::new(device.clone(), *baud_rate)
                        .data_bits(DataBits::Eight)
                        .parity(Parity::None)
                        .stop_bits(StopBits::One)
                        .flow_control(FlowControl::None)
                        .open_native_async()
                    {
                        Ok(port) => port,
                        Err(err) => {
                            log::warn!(
                                "failed to open LoRa serial device={} baud_rate={} err={}",
                                device,
                                baud_rate,
                                err
                            );
                            tokio::time::sleep(active_backoff).await;
                            active_backoff =
                                bounded_backoff_next(active_backoff, max_reconnect_backoff);
                            continue;
                        }
                    };

                    log::info!(
                        "opened LoRa serial device={} baud_rate={} iface={} frequency_hz={} bandwidth_hz={} sf={} cr={}",
                        device,
                        baud_rate,
                        iface_address,
                        config.frequency_hz,
                        config.bandwidth_hz,
                        config.spreading_factor,
                        config.coding_rate
                    );
                    active_backoff = reconnect_backoff;
                    run_lora_kiss_stream(
                        port,
                        LoraStreamRun {
                            interface: context.inner.clone(),
                            cancel: context.cancel.clone(),
                            iface_address,
                            endpoint_label: device.clone(),
                            config,
                            flow_control,
                            id_beacon: id_beacon.clone(),
                            activity_probe: None,
                            startup_response_timeout,
                            management_frame_rx: management_frame_rx.clone(),
                            rx_channel: rx_channel.clone(),
                            tx_channel: tx_channel.clone(),
                        },
                    )
                    .await;
                }
                LoraEndpoint::Tcp { addr } => {
                    let stream = match TcpStream::connect(addr.clone()).await {
                        Ok(stream) => stream,
                        Err(err) => {
                            log::warn!("failed to connect LoRa tcp addr={} err={}", addr, err);
                            tokio::time::sleep(active_backoff).await;
                            active_backoff =
                                bounded_backoff_next(active_backoff, max_reconnect_backoff);
                            continue;
                        }
                    };

                    log::info!(
                        "opened LoRa tcp addr={} iface={} frequency_hz={} bandwidth_hz={} sf={} cr={}",
                        addr,
                        iface_address,
                        config.frequency_hz,
                        config.bandwidth_hz,
                        config.spreading_factor,
                        config.coding_rate
                    );
                    active_backoff = reconnect_backoff;
                    run_lora_kiss_stream(
                        stream,
                        LoraStreamRun {
                            interface: context.inner.clone(),
                            cancel: context.cancel.clone(),
                            iface_address,
                            endpoint_label: addr.clone(),
                            config,
                            flow_control,
                            id_beacon: id_beacon.clone(),
                            activity_probe: Some(rnode_tcp_activity_probe()),
                            startup_response_timeout,
                            management_frame_rx: management_frame_rx.clone(),
                            rx_channel: rx_channel.clone(),
                            tx_channel: tx_channel.clone(),
                        },
                    )
                    .await;
                }
            };

            if context.cancel.is_cancelled() {
                break;
            }
            tokio::time::sleep(active_backoff).await;
            active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
        }

        iface_stop.cancel();
    }
}

fn rnode_management_channel() -> (RNodeManagementFrameSender, RNodeManagementFrameReceiver) {
    let (tx, rx) = tokio::sync::mpsc::channel(LORA_RNODE_MANAGEMENT_CHANNEL_CAPACITY);
    (tx, Arc::new(tokio::sync::Mutex::new(rx)))
}

#[derive(Debug, Clone)]
pub struct LoraRNodeManagementHandle {
    tx: RNodeManagementFrameSender,
    status: RNodeManagementStatus,
}

impl LoraRNodeManagementHandle {
    fn record_queued(&self, command: &str) {
        let mut status = self.status.lock().expect("lora management status mutex poisoned");
        status.record_queued(command);
    }

    fn record_failed(&self, command: &str, error: String) {
        let mut status = self.status.lock().expect("lora management status mutex poisoned");
        status.record_failed(command, error);
    }

    pub fn try_dispatch_named_frame(
        &self,
        command: &str,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<Vec<u8>>> {
        match self.tx.try_send(frame) {
            Ok(()) => {
                self.record_queued(command);
                Ok(())
            }
            Err(err) => {
                self.record_failed(command, err.to_string());
                Err(err)
            }
        }
    }

    pub fn try_dispatch_frame(
        &self,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<Vec<u8>>> {
        self.try_dispatch_named_frame("raw_frame", frame)
    }

    pub async fn dispatch_named_frame(
        &self,
        command: &str,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        match self.tx.send(frame).await {
            Ok(()) => {
                self.record_queued(command);
                Ok(())
            }
            Err(err) => {
                self.record_failed(command, err.to_string());
                Err(err)
            }
        }
    }

    pub async fn dispatch_frame(
        &self,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.dispatch_named_frame("raw_frame", frame).await
    }

    pub fn try_query_radio_state(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<Vec<u8>>> {
        self.try_dispatch_named_frame("radio_state_query", LoraConfig::radio_state_query_frame())
    }

    pub fn try_blink(&self, pattern: u8) -> Result<(), tokio::sync::mpsc::error::TrySendError<Vec<u8>>> {
        self.try_dispatch_named_frame("blink", LoraConfig::blink_frame(pattern))
    }

    pub async fn query_radio_state(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.dispatch_named_frame("radio_state_query", LoraConfig::radio_state_query_frame())
            .await
    }

    pub async fn blink(
        &self,
        pattern: u8,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.dispatch_named_frame("blink", LoraConfig::blink_frame(pattern)).await
    }
}

#[derive(Clone)]
pub struct LoraRuntimeStatusHandle {
    inner: Arc<std::sync::Mutex<LoraInterface>>,
}

impl LoraRuntimeStatusHandle {
    #[must_use]
    pub fn new(inner: Arc<std::sync::Mutex<LoraInterface>>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("lora interface mutex poisoned").runtime_status_json()
    }
}

struct LoraStreamRun {
    interface: Arc<std::sync::Mutex<LoraInterface>>,
    cancel: tokio_util::sync::CancellationToken,
    iface_address: crate::hash::AddressHash,
    endpoint_label: String,
    config: LoraConfig,
    flow_control: bool,
    id_beacon: Option<KissIdBeaconConfig>,
    activity_probe: Option<KissActivityProbeConfig>,
    startup_response_timeout: Duration,
    management_frame_rx: RNodeManagementFrameReceiver,
    rx_channel: tokio::sync::mpsc::Sender<crate::iface::RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::iface::TxMessage>>>,
}
