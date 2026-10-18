fn rnode_management_channel() -> (RNodeManagementFrameSender, RNodeManagementFrameReceiver) {
    let (tx, rx) = tokio::sync::mpsc::channel(LORA_RNODE_MANAGEMENT_CHANNEL_CAPACITY);
    (tx, Arc::new(tokio::sync::Mutex::new(rx)))
}

#[derive(Debug, Clone)]
pub struct LoraRNodeManagementHandle {
    tx: RNodeManagementFrameSender,
    status: RNodeManagementStatus,
    stored_config_image: RNodeStoredConfigImage,
}

impl LoraRNodeManagementHandle {
    /// See [`LoraInterface::stored_config`]. A `rom_read` dispatch only asks
    /// the radio; this is where the answer arrives.
    pub fn stored_config(&self) -> Result<Option<StoredRadioConfig>, StoredConfigError> {
        stored_config_from_image(&self.stored_config_image)
    }

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

    pub fn try_blink(
        &self,
        pattern: u8,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<Vec<u8>>> {
        self.try_dispatch_named_frame("blink", LoraConfig::blink_frame(pattern))
    }

    pub async fn query_radio_state(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.dispatch_named_frame("radio_state_query", LoraConfig::radio_state_query_frame()).await
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
    ifac_state: IfacState,
    ifac_violations: Arc<std::sync::atomic::AtomicU64>,
}
