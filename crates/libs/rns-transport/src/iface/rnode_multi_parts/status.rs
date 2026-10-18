#[derive(Debug, Clone)]
pub(crate) struct RNodeMultiStreamOptions {
    parent_iface: AddressHash,
    device: String,
    subinterfaces: Vec<RNodeMultiSubInterfaceConfig>,
    runtime_status: Arc<Mutex<RNodeMultiRuntimeStatus>>,
    vport_map: BTreeMap<AddressHash, u8>,
    mtu: usize,
    startup_probe: Option<RNodeMultiStartupProbe>,
    initial_frames: Vec<Vec<u8>>,
    shutdown_frames: Vec<Vec<u8>>,
    id_beacon: Option<KissIdBeaconConfig>,
    management_frame_rx: RNodeMultiManagementFrameReceiver,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RNodeMultiRuntimeStatus {
    pub stream_state: String,
    pub last_error: Option<String>,
    pub selected_vport: u8,
    pub startup_probe: Option<RNodeMultiProbeStatus>,
    pub radio_status: BTreeMap<u8, RNodeRadioStatus>,
}

#[derive(Clone)]
pub struct RNodeMultiRuntimeStatusHandle {
    inner: Arc<Mutex<RNodeMultiRuntimeStatus>>,
}

impl RNodeMultiRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("rnode multi runtime status mutex poisoned").to_json()
    }
}

fn rnode_multi_management_channel(
) -> (RNodeMultiManagementFrameSender, RNodeMultiManagementFrameReceiver) {
    let (tx, rx) = tokio::sync::mpsc::channel(RNODE_MULTI_MANAGEMENT_CHANNEL_CAPACITY);
    (tx, Arc::new(tokio::sync::Mutex::new(rx)))
}

#[derive(Debug, Clone)]
pub struct RNodeMultiManagementHandle {
    tx: RNodeMultiManagementFrameSender,
}

impl RNodeMultiManagementHandle {
    pub fn try_dispatch_frame(
        &self,
        vport: u8,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<(u8, Vec<u8>)>> {
        self.tx.try_send((vport, frame))
    }

    pub async fn dispatch_frame(
        &self,
        vport: u8,
        frame: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<(u8, Vec<u8>)>> {
        self.tx.send((vport, frame)).await
    }
}

impl RNodeMultiRuntimeStatus {
    #[must_use]
    pub fn from_subinterfaces(subinterfaces: &[RNodeMultiSubInterfaceConfig]) -> Self {
        let mut radio_status = BTreeMap::new();
        for subinterface in subinterfaces {
            radio_status.entry(subinterface.vport).or_default();
        }
        Self {
            stream_state: "configured".to_string(),
            last_error: None,
            selected_vport: subinterfaces.first().map_or(0, |subinterface| subinterface.vport),
            startup_probe: None,
            radio_status,
        }
    }

    pub fn set_stream_state(&mut self, state: &str, last_error: Option<String>) {
        self.stream_state = state.to_string();
        self.last_error = last_error;
    }

    pub fn accept_command(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        if command == CMD_SEL_INT {
            let [vport] = payload else {
                return Err(
                    "rnode multi selected-interface response must contain one byte".to_string()
                );
            };
            self.selected_vport = *vport;
            self.radio_status.entry(*vport).or_default();
            return Ok(true);
        }

        let status = self.radio_status.entry(self.selected_vport).or_default();
        accept_rnode_multi_radio_status_command(status, command, payload)
    }

    pub fn set_startup_probe(&mut self, status: RNodeMultiProbeStatus) {
        self.startup_probe = Some(status);
    }

    #[must_use]
    pub fn status_for_vport(&self, vport: u8) -> Option<&RNodeRadioStatus> {
        self.radio_status.get(&vport)
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert(
            "stream_state".to_string(),
            serde_json::Value::String(self.stream_state.clone()),
        );
        root.insert(
            "last_error".to_string(),
            self.last_error
                .as_ref()
                .map(|value| serde_json::Value::String(value.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "selected_vport".to_string(),
            serde_json::Value::Number(u64::from(self.selected_vport).into()),
        );
        root.insert(
            "startup_probe".to_string(),
            self.startup_probe
                .as_ref()
                .map(RNodeMultiProbeStatus::to_json)
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "vports".to_string(),
            serde_json::Value::Array(
                self.radio_status
                    .keys()
                    .map(|vport| serde_json::Value::Number(u64::from(*vport).into()))
                    .collect(),
            ),
        );
        root.insert(
            "subinterfaces".to_string(),
            serde_json::Value::Object(
                self.radio_status
                    .iter()
                    .map(|(vport, status)| (vport.to_string(), status.to_json()))
                    .collect(),
            ),
        );
        serde_json::Value::Object(root)
    }
}

fn update_rnode_multi_runtime_state(
    status: &Arc<Mutex<RNodeMultiRuntimeStatus>>,
    state: &str,
    last_error: Option<String>,
) {
    status
        .lock()
        .expect("rnode multi runtime status mutex poisoned")
        .set_stream_state(state, last_error);
}

fn accept_rnode_multi_radio_status_command(
    status: &mut RNodeRadioStatus,
    command: u8,
    payload: &[u8],
) -> Result<bool, String> {
    match command {
        CMD_STAT_CHTM if payload.len() == 8 => {
            let [ats_hi, ats_lo, atl_hi, atl_lo, cus_hi, cus_lo, cul_hi, cul_lo] = payload else {
                unreachable!("length checked above");
            };
            status.airtime_short_percent =
                Some(f64::from(u16::from_be_bytes([*ats_hi, *ats_lo])) / 100.0);
            status.airtime_long_percent =
                Some(f64::from(u16::from_be_bytes([*atl_hi, *atl_lo])) / 100.0);
            status.channel_load_short_percent =
                Some(f64::from(u16::from_be_bytes([*cus_hi, *cus_lo])) / 100.0);
            status.channel_load_long_percent =
                Some(f64::from(u16::from_be_bytes([*cul_hi, *cul_lo])) / 100.0);
            Ok(true)
        }
        CMD_STAT_PHYPRM if payload.len() == 10 => {
            let [lst_hi, lst_lo, lsr_hi, lsr_lo, prs_hi, prs_lo, prt_hi, prt_lo, cst_hi, cst_lo] =
                payload
            else {
                unreachable!("length checked above");
            };
            status.symbol_time_ms =
                Some(f64::from(u16::from_be_bytes([*lst_hi, *lst_lo])) / 1000.0);
            status.symbol_rate_baud = Some(u16::from_be_bytes([*lsr_hi, *lsr_lo]));
            status.preamble_symbols = Some(u16::from_be_bytes([*prs_hi, *prs_lo]));
            status.preamble_time_ms = Some(u16::from_be_bytes([*prt_hi, *prt_lo]));
            status.csma_slot_time_ms = Some(u16::from_be_bytes([*cst_hi, *cst_lo]));
            Ok(true)
        }
        _ => status.accept_command(command, payload),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RNodeMultiInterfaceType {
    Sx126x,
    Sx127x,
    Sx128x,
    Unknown(u8),
}

impl RNodeMultiInterfaceType {
    #[must_use]
    pub const fn from_byte(value: u8) -> Self {
        match value {
            0x10 | 0x11 => Self::Sx126x,
            0x00..=0x02 => Self::Sx127x,
            0x20 | 0x21 => Self::Sx128x,
            value => Self::Unknown(value),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sx126x => "SX126X",
            Self::Sx127x => "SX127X",
            Self::Sx128x => "SX128X",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RNodeMultiProbeStatus {
    pub detected: bool,
    pub firmware_version: Option<(u8, u8)>,
    pub platform: Option<u8>,
    pub mcu: Option<u8>,
    pub interfaces: BTreeMap<u8, RNodeMultiInterfaceType>,
}

impl RNodeMultiProbeStatus {
    pub fn accept_command(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        match command {
            CMD_DETECT => {
                let [value] = payload else {
                    return Err("rnode multi detect response must contain one byte".to_string());
                };
                self.detected = *value == DETECT_RESP;
                Ok(true)
            }
            CMD_FW_VERSION => {
                let [major, minor] = payload else {
                    return Err("rnode multi firmware response must contain two bytes".to_string());
                };
                self.firmware_version = Some((*major, *minor));
                Ok(true)
            }
            CMD_PLATFORM => {
                let [platform] = payload else {
                    return Err("rnode multi platform response must contain one byte".to_string());
                };
                self.platform = Some(*platform);
                Ok(true)
            }
            CMD_MCU => {
                let [mcu] = payload else {
                    return Err("rnode multi mcu response must contain one byte".to_string());
                };
                self.mcu = Some(*mcu);
                Ok(true)
            }
            CMD_INTERFACES => {
                if payload.is_empty() || !payload.len().is_multiple_of(2) {
                    return Err(
                        "rnode multi interfaces response must contain two-byte records".to_string()
                    );
                }
                for record in payload.as_chunks::<2>().0 {
                    let vport = record[0];
                    let kind = RNodeMultiInterfaceType::from_byte(record[1]);
                    if self.interfaces.insert(vport, kind).is_some() {
                        return Err(format!(
                            "rnode multi interfaces response repeated vport {vport}"
                        ));
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn validate_startup_probe(&self, required_vports: &[u8]) -> Result<(), String> {
        if !self.detected {
            return Err("rnode multi detect response did not confirm an RNode device".to_string());
        }
        let Some((major, minor)) = self.firmware_version else {
            return Err("rnode multi firmware response is missing".to_string());
        };
        if major < RNODE_MULTI_REQUIRED_FW_VERSION_MAJOR
            || (major == RNODE_MULTI_REQUIRED_FW_VERSION_MAJOR
                && minor < RNODE_MULTI_REQUIRED_FW_VERSION_MINOR)
        {
            return Err(format!(
                "rnode multi firmware version {major}.{minor} is below required {RNODE_MULTI_REQUIRED_FW_VERSION_MAJOR}.{RNODE_MULTI_REQUIRED_FW_VERSION_MINOR}"
            ));
        }
        if self.platform.is_none() {
            return Err("rnode multi platform response is missing".to_string());
        }
        if self.mcu.is_none() {
            return Err("rnode multi mcu response is missing".to_string());
        }
        if self.interfaces.is_empty() {
            return Err("rnode multi interfaces response is missing".to_string());
        }
        for vport in required_vports {
            if !self.interfaces.contains_key(vport) {
                return Err(format!(
                    "rnode multi configured vport {vport} was not reported by hardware"
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn interface_summary(&self) -> String {
        self.interfaces
            .iter()
            .map(|(vport, kind)| format!("{vport}:{}", kind.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        root.insert("detected".to_string(), serde_json::Value::Bool(self.detected));
        root.insert(
            "firmware_version".to_string(),
            self.firmware_version.map_or(serde_json::Value::Null, |(major, minor)| {
                serde_json::json!({
                    "major": major,
                    "minor": minor,
                    "label": format!("{major}.{minor:02}"),
                })
            }),
        );
        root.insert(
            "platform".to_string(),
            self.platform
                .map(|value| serde_json::Value::Number(u64::from(value).into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "mcu".to_string(),
            self.mcu
                .map(|value| serde_json::Value::Number(u64::from(value).into()))
                .unwrap_or(serde_json::Value::Null),
        );
        root.insert(
            "interfaces".to_string(),
            serde_json::Value::Object(
                self.interfaces
                    .iter()
                    .map(|(vport, kind)| {
                        (vport.to_string(), serde_json::Value::String(kind.as_str().to_string()))
                    })
                    .collect(),
            ),
        );
        root.insert(
            "interface_summary".to_string(),
            serde_json::Value::String(self.interface_summary()),
        );
        serde_json::Value::Object(root)
    }

    #[must_use]
    pub fn has_display(&self) -> bool {
        matches!(self.platform, Some(PLATFORM_ESP32 | PLATFORM_NRF52))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RNodeMultiStartupProbe {
    frames: Vec<Vec<u8>>,
    required_vports: Vec<u8>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
struct RNodeMultiStartupProbeError {
    message: String,
    status: RNodeMultiProbeStatus,
}

impl RNodeMultiStartupProbeError {
    fn new(message: impl Into<String>, status: RNodeMultiProbeStatus) -> Self {
        Self { message: message.into(), status }
    }

    fn is_cancel_or_stop(&self) -> bool {
        self.message == "startup cancelled" || self.message == "interface stopped"
    }
}

impl RNodeMultiStartupProbe {
    fn from_subinterfaces(subinterfaces: &[RNodeMultiSubInterfaceConfig]) -> Self {
        let mut required_vports = subinterfaces.iter().map(|sub| sub.vport).collect::<Vec<_>>();
        required_vports.sort_unstable();
        required_vports.dedup();
        Self {
            frames: rnode_multi_probe_frames(),
            required_vports,
            timeout: RNODE_MULTI_STARTUP_RESPONSE_TIMEOUT,
        }
    }
}

#[cfg(test)]
pub(crate) async fn run_rnode_multi_stream<IO>(
    stream: IO,
    options: RNodeMultiStreamOptions,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    run_rnode_multi_stream_with_ifac(
        stream,
        options,
        cancel,
        iface_stop,
        rx_channel,
        tx_channel,
        IfacRuntime::from_parts(
            Arc::new(std::sync::RwLock::new(None)),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        ),
    )
    .await;
}
