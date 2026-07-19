use alloc::string::String;
use std::collections::BTreeMap;
use std::net::{TcpStream as StdTcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::sync::CancellationToken;

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::hash::AddressHash;
use crate::iface::kiss::{KissIdBeaconConfig, KISS_READ_FRAME_TIMEOUT};
use crate::kiss::{encode_command_frame, KissCommand, KissFrame, KissStreamDecoder};
use crate::packet::Packet;
use crate::serde::Serialize;

use super::lora::{
    LoraConfig, RNodeRadioStatus, CMD_DETECT, CMD_FB_EXT, CMD_FW_VERSION, CMD_LEAVE, CMD_MCU,
    CMD_PLATFORM, CMD_RADIO_STATE, CMD_STAT_CHTM, CMD_STAT_PHYPRM, DETECT_REQ, DETECT_RESP,
    PLATFORM_ESP32, PLATFORM_NRF52, RADIO_STATE_OFF, RADIO_STATE_ON,
};
use super::{
    IfaceRole, IfaceSource, Interface, InterfaceContext, InterfaceManager, RxMessage, TxMessage,
    TxMessageType,
};

const CMD_SEL_INT: u8 = 0x1F;
pub const CMD_INTERFACES: u8 = 0x71;
const DEFAULT_BAUD_RATE: u32 = 115_200;
const DEFAULT_MTU: usize = 508;
const RECONNECT_WAIT: Duration = Duration::from_secs(5);
const RNODE_MULTI_REQUIRED_FW_VERSION_MAJOR: u8 = 1;
const RNODE_MULTI_REQUIRED_FW_VERSION_MINOR: u8 = 74;
const RNODE_MULTI_STARTUP_RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_500);
const RNODE_MULTI_MANAGEMENT_CHANNEL_CAPACITY: usize = 64;

type RNodeMultiManagementFrameSender = tokio::sync::mpsc::Sender<(u8, Vec<u8>)>;
type RNodeMultiManagementFrameReceiver =
    Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<(u8, Vec<u8>)>>>;

#[derive(Debug, Clone)]
pub struct RNodeMultiSubInterfaceConfig {
    pub name: String,
    pub vport: u8,
    pub config: LoraConfig,
    pub outgoing: bool,
}

#[derive(Clone)]
pub struct RNodeMultiInterface {
    endpoint: RNodeMultiEndpoint,
    subinterfaces: Vec<RNodeMultiSubInterfaceConfig>,
    id_beacon: Option<KissIdBeaconConfig>,
    mtu: usize,
    runtime_status: Arc<Mutex<RNodeMultiRuntimeStatus>>,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    management_frame_tx: RNodeMultiManagementFrameSender,
    management_frame_rx: RNodeMultiManagementFrameReceiver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RNodeMultiEndpoint {
    Serial { device: String, baud_rate: u32 },
    Tcp { addr: String },
}

impl RNodeMultiEndpoint {
    fn label(&self) -> &str {
        match self {
            Self::Serial { device, .. } => device,
            Self::Tcp { addr } => addr,
        }
    }

    fn baud_rate(&self) -> Option<u32> {
        match self {
            Self::Serial { baud_rate, .. } => Some(*baud_rate),
            Self::Tcp { .. } => None,
        }
    }
}

impl RNodeMultiInterface {
    #[must_use]
    pub fn new<T: Into<String>>(
        device: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_multi_management_channel();
        Self {
            endpoint: RNodeMultiEndpoint::Serial {
                device: device.into(),
                baud_rate: DEFAULT_BAUD_RATE,
            },
            subinterfaces: Vec::new(),
            id_beacon: None,
            mtu: DEFAULT_MTU,
            runtime_status: Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&[]))),
            iface_manager,
            management_frame_tx,
            management_frame_rx,
        }
    }

    #[must_use]
    pub fn new_tcp<T: Into<String>>(
        addr: T,
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    ) -> Self {
        let (management_frame_tx, management_frame_rx) = rnode_multi_management_channel();
        Self {
            endpoint: RNodeMultiEndpoint::Tcp { addr: addr.into() },
            subinterfaces: Vec::new(),
            id_beacon: None,
            mtu: DEFAULT_MTU,
            runtime_status: Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&[]))),
            iface_manager,
            management_frame_tx,
            management_frame_rx,
        }
    }

    #[must_use]
    pub fn with_baud_rate(mut self, baud_rate: u32) -> Self {
        if let RNodeMultiEndpoint::Serial { baud_rate: current, .. } = &mut self.endpoint {
            *current = baud_rate.max(1);
        }
        self
    }

    #[must_use]
    pub fn with_subinterfaces(mut self, subinterfaces: Vec<RNodeMultiSubInterfaceConfig>) -> Self {
        self.runtime_status =
            Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(&subinterfaces)));
        self.subinterfaces = subinterfaces;
        self
    }

    #[must_use]
    pub fn with_id_beacon(mut self, id_beacon: Option<KissIdBeaconConfig>) -> Self {
        self.id_beacon = id_beacon;
        self
    }

    #[must_use]
    pub fn with_mtu(mut self, mtu: usize) -> Self {
        self.mtu = mtu.max(256);
        self
    }

    #[must_use]
    pub fn subinterfaces(&self) -> &[RNodeMultiSubInterfaceConfig] {
        &self.subinterfaces
    }

    #[must_use]
    pub fn id_beacon(&self) -> Option<&KissIdBeaconConfig> {
        self.id_beacon.as_ref()
    }

    #[must_use]
    pub fn mtu_value(&self) -> usize {
        self.mtu
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
    pub fn runtime_status_handle(&self) -> RNodeMultiRuntimeStatusHandle {
        RNodeMultiRuntimeStatusHandle { inner: self.runtime_status.clone() }
    }

    #[must_use]
    pub fn rnode_management_handle(&self) -> RNodeMultiManagementHandle {
        RNodeMultiManagementHandle { tx: self.management_frame_tx.clone() }
    }

    pub fn preflight_open(&self) -> Result<(), String> {
        match &self.endpoint {
            RNodeMultiEndpoint::Serial { device, baud_rate } => {
                tokio_serial::new(device.clone(), *baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(FlowControl::None)
                    .open_native_async()
                    .map(|_| ())
                    .map_err(|err| {
                        format!(
                            "rnode_multi preflight open failed device={} baud_rate={} err={}",
                            device, baud_rate, err
                        )
                    })
            }
            RNodeMultiEndpoint::Tcp { addr } => preflight_tcp_connect(addr),
        }
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let parent_iface = context.channel.address;
        let (
            endpoint,
            subinterfaces,
            id_beacon,
            mtu,
            runtime_status,
            iface_manager,
            management_frame_rx,
        ) = {
            let guard = context.inner.lock().expect("rnode multi interface mutex poisoned");
            (
                guard.endpoint.clone(),
                guard.subinterfaces.clone(),
                guard.id_beacon.clone(),
                guard.mtu,
                guard.runtime_status.clone(),
                guard.iface_manager.clone(),
                guard.management_frame_rx.clone(),
            )
        };
        let mut vport_map = BTreeMap::new();
        {
            let mut manager = iface_manager.lock().await;
            for subinterface in &subinterfaces {
                if let Some(address) =
                    manager.register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
                {
                    manager.set_outgoing(address, subinterface.outgoing);
                    vport_map.insert(address, subinterface.vport);
                }
            }
        }

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));

        loop {
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }

            update_rnode_multi_runtime_state(&runtime_status, "opening", None);
            match &endpoint {
                RNodeMultiEndpoint::Serial { device, baud_rate } => {
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
                                "failed to open RNodeMulti serial device={} baud_rate={} err={}",
                                device,
                                baud_rate,
                                err
                            );
                            update_rnode_multi_runtime_state(
                                &runtime_status,
                                "open_failed",
                                Some(err.to_string()),
                            );
                            tokio::select! {
                                _ = context.cancel.cancelled() => break,
                                _ = iface_stop.cancelled() => break,
                                _ = tokio::time::sleep(RECONNECT_WAIT) => {}
                            }
                            continue;
                        }
                    };

                    log::info!(
                        "opened RNodeMulti serial device={} baud_rate={} iface={} subinterfaces={}",
                        device,
                        baud_rate,
                        parent_iface,
                        subinterfaces.len()
                    );

                    run_rnode_multi_stream(
                        port,
                        rnode_multi_stream_options(
                            parent_iface,
                            device.clone(),
                            &subinterfaces,
                            mtu,
                            RNodeMultiStreamRuntime {
                                status: runtime_status.clone(),
                                management_frame_rx: management_frame_rx.clone(),
                            },
                            vport_map.clone(),
                            id_beacon.clone(),
                        ),
                        context.cancel.clone(),
                        iface_stop.clone(),
                        rx_channel.clone(),
                        tx_channel.clone(),
                    )
                    .await;
                }
                RNodeMultiEndpoint::Tcp { addr } => {
                    let stream = match TcpStream::connect(addr.clone()).await {
                        Ok(stream) => stream,
                        Err(err) => {
                            log::warn!(
                                "failed to connect RNodeMulti tcp addr={} err={}",
                                addr,
                                err
                            );
                            update_rnode_multi_runtime_state(
                                &runtime_status,
                                "open_failed",
                                Some(err.to_string()),
                            );
                            tokio::select! {
                                _ = context.cancel.cancelled() => break,
                                _ = iface_stop.cancelled() => break,
                                _ = tokio::time::sleep(RECONNECT_WAIT) => {}
                            }
                            continue;
                        }
                    };

                    log::info!(
                        "opened RNodeMulti tcp addr={} iface={} subinterfaces={}",
                        addr,
                        parent_iface,
                        subinterfaces.len()
                    );

                    run_rnode_multi_stream(
                        stream,
                        rnode_multi_stream_options(
                            parent_iface,
                            addr.clone(),
                            &subinterfaces,
                            mtu,
                            RNodeMultiStreamRuntime {
                                status: runtime_status.clone(),
                                management_frame_rx: management_frame_rx.clone(),
                            },
                            vport_map.clone(),
                            id_beacon.clone(),
                        ),
                        context.cancel.clone(),
                        iface_stop.clone(),
                        rx_channel.clone(),
                        tx_channel.clone(),
                    )
                    .await;
                }
            }
        }

        update_rnode_multi_runtime_state(&runtime_status, "stopped", None);
        iface_stop.cancel();
        cleanup_rnode_multi_virtual_ifaces(&iface_manager, &vport_map).await;
    }
}

fn preflight_tcp_connect(addr: &str) -> Result<(), String> {
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| format!("rnode_multi tcp preflight resolve failed addr={addr} err={err}"))?
        .next()
        .ok_or_else(|| format!("rnode_multi tcp preflight resolve failed addr={addr}"))?;
    StdTcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
        .map(|_| ())
        .map_err(|err| format!("rnode_multi tcp preflight connect failed addr={addr} err={err}"))
}

async fn cleanup_rnode_multi_virtual_ifaces(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    vport_map: &BTreeMap<AddressHash, u8>,
) {
    if vport_map.is_empty() {
        return;
    }

    let mut manager = iface_manager.lock().await;
    for iface in vport_map.keys() {
        if !manager.stop_interface(*iface) {
            log::debug!("RNodeMulti virtual interface already absent iface={iface}");
        }
    }
}

fn rnode_multi_stream_options(
    parent_iface: AddressHash,
    device: String,
    subinterfaces: &[RNodeMultiSubInterfaceConfig],
    mtu: usize,
    runtime: RNodeMultiStreamRuntime,
    vport_map: BTreeMap<AddressHash, u8>,
    id_beacon: Option<KissIdBeaconConfig>,
) -> RNodeMultiStreamOptions {
    RNodeMultiStreamOptions {
        parent_iface,
        device,
        subinterfaces: subinterfaces.to_vec(),
        runtime_status: runtime.status,
        vport_map,
        mtu,
        startup_probe: Some(RNodeMultiStartupProbe::from_subinterfaces(subinterfaces)),
        initial_frames: rnode_multi_initial_frames(subinterfaces),
        shutdown_frames: rnode_multi_shutdown_frames(subinterfaces),
        id_beacon,
        management_frame_rx: runtime.management_frame_rx,
    }
}

struct RNodeMultiStreamRuntime {
    status: Arc<Mutex<RNodeMultiRuntimeStatus>>,
    management_frame_rx: RNodeMultiManagementFrameReceiver,
}

impl Interface for RNodeMultiInterface {
    fn mtu() -> usize {
        DEFAULT_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.mtu
    }
}

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
                if payload.is_empty() || payload.len() % 2 != 0 {
                    return Err(
                        "rnode multi interfaces response must contain two-byte records".to_string()
                    );
                }
                for record in payload.chunks_exact(2) {
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

pub(crate) async fn run_rnode_multi_stream<IO>(
    mut stream: IO,
    options: RNodeMultiStreamOptions,
    cancel: CancellationToken,
    iface_stop: CancellationToken,
    rx_channel: tokio::sync::mpsc::Sender<RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<TxMessage>>>,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut decoder = KissStreamDecoder::new(options.mtu.max(256));
    let mut read_buffer = vec![0_u8; options.mtu.max(256)];
    let mut display_capable = false;
    {
        let mut status =
            options.runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
        *status = RNodeMultiRuntimeStatus::from_subinterfaces(&options.subinterfaces);
        status.set_stream_state("probing", None);
    }

    if let Some(probe) = options.startup_probe.as_ref() {
        match run_rnode_multi_startup_probe(
            &mut stream,
            probe,
            &mut decoder,
            &mut read_buffer,
            &cancel,
            &iface_stop,
        )
        .await
        {
            Ok(status) => {
                display_capable = status.has_display();
                options
                    .runtime_status
                    .lock()
                    .expect("rnode multi runtime status mutex poisoned")
                    .set_startup_probe(status.clone());
                log::info!(
                    "RNodeMulti startup probe accepted iface={} device={} firmware={:?} platform={:?} mcu={:?} interfaces={}",
                    options.parent_iface,
                    options.device,
                    status.firmware_version,
                    status.platform,
                    status.mcu,
                    status.interface_summary()
                );
            }
            Err(err) => {
                log::warn!(
                    "RNodeMulti startup probe failed iface={} device={} err={}",
                    options.parent_iface,
                    options.device,
                    err.message
                );
                if err.is_cancel_or_stop() {
                    update_rnode_multi_runtime_state(&options.runtime_status, "closed", None);
                } else {
                    let message = err.message;
                    options
                        .runtime_status
                        .lock()
                        .expect("rnode multi runtime status mutex poisoned")
                        .set_startup_probe(err.status);
                    update_rnode_multi_runtime_state(
                        &options.runtime_status,
                        "probe_failed",
                        Some(message),
                    );
                }
                return;
            }
        }
    }

    for frame in &options.initial_frames {
        if let Err(err) = stream.write_all(frame).await {
            log::warn!(
                "RNodeMulti init write error iface={} device={} err={}",
                options.parent_iface,
                options.device,
                err
            );
            update_rnode_multi_runtime_state(
                &options.runtime_status,
                "init_failed",
                Some(err.to_string()),
            );
            return;
        }
    }
    if let Err(err) = stream.flush().await {
        log::warn!(
            "RNodeMulti init flush error iface={} device={} err={}",
            options.parent_iface,
            options.device,
            err
        );
        update_rnode_multi_runtime_state(
            &options.runtime_status,
            "init_failed",
            Some(err.to_string()),
        );
        return;
    }

    let mut tx_buffer = vec![0_u8; options.mtu];
    let mut last_read_at = tokio::time::Instant::now();
    let mut first_tx_at: Option<tokio::time::Instant> = None;
    let mut id_tick = tokio::time::interval(Duration::from_millis(250));
    let mut mark_closed_on_exit = false;
    update_rnode_multi_runtime_state(&options.runtime_status, "running", None);

    'stream_loop: loop {
        let mut tx_channel = tx_channel.lock().await;
        let mut management_frame_rx = options.management_frame_rx.lock().await;
        tokio::select! {
            _ = cancel.cancelled() => {
                mark_closed_on_exit = true;
                break;
            }
            _ = iface_stop.cancelled() => {
                mark_closed_on_exit = true;
                break;
            }
            _ = id_tick.tick(), if options.id_beacon.is_some() && first_tx_at.is_some() => {
                let Some(beacon) = options.id_beacon.as_ref() else {
                    continue;
                };
                let Some(first_tx) = first_tx_at else {
                    continue;
                };
                if first_tx.elapsed() >= beacon.interval
                    && write_rnode_multi_id_beacon(&mut stream, &options, beacon).await
                {
                    first_tx_at = None;
                }
            }
            result = stream.read(&mut read_buffer[..]) => {
                match result {
                    Ok(0) => {
                        mark_closed_on_exit = true;
                        break;
                    }
                    Ok(n) => {
                        if decoder.has_partial_frame()
                            && last_read_at.elapsed() >= KISS_READ_FRAME_TIMEOUT
                        {
                            decoder.clear_partial_frame();
                        }
                        last_read_at = tokio::time::Instant::now();
                        match decoder.push_bytes(&read_buffer[..n]) {
                            Ok(frames) => {
                                for frame in frames {
                                    process_rnode_multi_frame(
                                        frame,
                                        &options,
                                        &rx_channel,
                                    )
                                    .await;
                                }
                            }
                            Err(err) => log::warn!(
                                "RNodeMulti KISS decode error iface={} device={} err={:?}",
                                options.parent_iface,
                                options.device,
                                err
                            ),
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "RNodeMulti read error iface={} device={} err={}",
                            options.parent_iface,
                            options.device,
                            err
                        );
                        update_rnode_multi_runtime_state(
                            &options.runtime_status,
                            "read_failed",
                            Some(err.to_string()),
                        );
                        break;
                    }
                }
            }
            Some(message) = tx_channel.recv() => {
                let Some(vports) = rnode_multi_tx_vports(&message, &options) else {
                    continue;
                };
                let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                if message.packet.serialize(&mut output).is_err() {
                    log::warn!(
                        "RNodeMulti packet serialize failed iface={} device={} mtu={}",
                        options.parent_iface,
                        options.device,
                        options.mtu
                    );
                    continue;
                }
                for vport in vports {
                    if !write_rnode_multi_data(&mut stream, vport, output.as_slice()).await {
                        log::warn!(
                            "RNodeMulti data frame write failed iface={} device={} vport={}",
                            options.parent_iface,
                            options.device,
                            vport
                        );
                        update_rnode_multi_runtime_state(
                            &options.runtime_status,
                            "write_failed",
                            Some("data frame write failed".to_string()),
                        );
                        break 'stream_loop;
                    }
                }
                if first_tx_at.is_none() {
                    first_tx_at = Some(tokio::time::Instant::now());
                }
            }
            Some((vport, frame)) = management_frame_rx.recv() => {
                if !write_rnode_multi_management_frame(&mut stream, vport, &frame).await {
                    log::warn!(
                        "RNodeMulti management frame write failed iface={} device={} vport={}",
                        options.parent_iface,
                        options.device,
                        vport
                    );
                    update_rnode_multi_runtime_state(
                        &options.runtime_status,
                        "write_failed",
                        Some("management frame write failed".to_string()),
                    );
                    break;
                }
                if first_tx_at.is_none() {
                    first_tx_at = Some(tokio::time::Instant::now());
                }
            }
        }
    }

    let mut shutdown_error = None;
    if display_capable {
        if let Err(error) = stream.write_all(&rnode_multi_external_framebuffer_frame(false)).await {
            shutdown_error = Some(format!("external framebuffer shutdown write failed: {error}"));
        }
    }
    if shutdown_error.is_none() {
        for frame in &options.shutdown_frames {
            if let Err(error) = stream.write_all(frame).await {
                shutdown_error = Some(format!("shutdown frame write failed: {error}"));
                break;
            }
        }
    }
    if shutdown_error.is_none() {
        if let Err(error) = stream.flush().await {
            shutdown_error = Some(format!("shutdown flush failed: {error}"));
        }
    }
    if let Some(error) = shutdown_error {
        log::warn!(
            "RNodeMulti shutdown error iface={} device={} error={error}",
            options.parent_iface,
            options.device
        );
        update_rnode_multi_runtime_state(&options.runtime_status, "shutdown_failed", Some(error));
    } else if mark_closed_on_exit {
        update_rnode_multi_runtime_state(&options.runtime_status, "closed", None);
    }
}

async fn run_rnode_multi_startup_probe<IO>(
    stream: &mut IO,
    probe: &RNodeMultiStartupProbe,
    decoder: &mut KissStreamDecoder,
    read_buffer: &mut [u8],
    cancel: &CancellationToken,
    iface_stop: &CancellationToken,
) -> Result<RNodeMultiProbeStatus, RNodeMultiStartupProbeError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let mut status = RNodeMultiProbeStatus::default();
    for frame in &probe.frames {
        stream.write_all(frame).await.map_err(|err| {
            RNodeMultiStartupProbeError::new(format!("probe write failed: {err}"), status.clone())
        })?;
    }
    stream.flush().await.map_err(|err| {
        RNodeMultiStartupProbeError::new(format!("probe flush failed: {err}"), status.clone())
    })?;

    let deadline = tokio::time::Instant::now() + probe.timeout;

    loop {
        if status.validate_startup_probe(&probe.required_vports).is_ok() {
            return Ok(status);
        }

        tokio::select! {
            _ = cancel.cancelled() => {
                return Err(RNodeMultiStartupProbeError::new("startup cancelled", status));
            }
            _ = iface_stop.cancelled() => {
                return Err(RNodeMultiStartupProbeError::new("interface stopped", status));
            }
            _ = tokio::time::sleep_until(deadline) => {
                let message = status
                    .validate_startup_probe(&probe.required_vports)
                    .unwrap_err();
                return Err(RNodeMultiStartupProbeError::new(message, status));
            }
            result = stream.read(read_buffer) => {
                let n = result.map_err(|err| {
                    RNodeMultiStartupProbeError::new(
                        format!("probe read failed: {err}"),
                        status.clone(),
                    )
                })?;
                if n == 0 {
                    return Err(RNodeMultiStartupProbeError::new(
                        "probe stream closed",
                        status,
                    ));
                }
                let frames = decoder
                    .push_bytes(&read_buffer[..n])
                    .map_err(|err| {
                        RNodeMultiStartupProbeError::new(
                            format!("probe KISS decode failed: {err:?}"),
                            status.clone(),
                        )
                    })?;
                for frame in frames {
                    if let KissFrame::Command(KissCommand::Unknown(command, payload)) = frame {
                        status.accept_command(command, &payload).map_err(|err| {
                            RNodeMultiStartupProbeError::new(err, status.clone())
                        })?;
                    }
                }
            }
        }
    }
}

async fn process_rnode_multi_frame(
    frame: KissFrame,
    options: &RNodeMultiStreamOptions,
    rx_channel: &tokio::sync::mpsc::Sender<RxMessage>,
) {
    match frame {
        KissFrame::Data(payload) => {
            process_rnode_multi_payload(0, &payload, options, rx_channel).await;
        }
        KissFrame::Command(KissCommand::Unknown(command, payload)) => {
            if let Some(vport) = rnode_multi_data_command_vport(command) {
                process_rnode_multi_payload(vport, &payload, options, rx_channel).await;
            } else {
                let mut runtime_status = options
                    .runtime_status
                    .lock()
                    .expect("rnode multi runtime status mutex poisoned");
                match runtime_status.accept_command(command, &payload) {
                    Ok(true) => log::debug!(
                        "RNodeMulti status updated iface={} device={} vport={} command=0x{:02x}",
                        options.parent_iface,
                        options.device,
                        runtime_status.selected_vport,
                        command
                    ),
                    Ok(false) => {}
                    Err(err) => log::warn!(
                        "RNodeMulti status response rejected iface={} device={} vport={} command=0x{:02x} err={}",
                        options.parent_iface,
                        options.device,
                        runtime_status.selected_vport,
                        command,
                        err
                    ),
                }
            }
        }
        KissFrame::Command(KissCommand::Ready) => {}
    }
}

async fn process_rnode_multi_payload(
    vport: u8,
    payload: &[u8],
    options: &RNodeMultiStreamOptions,
    rx_channel: &tokio::sync::mpsc::Sender<RxMessage>,
) {
    let Some(address) = options
        .vport_map
        .iter()
        .find_map(|(address, mapped)| (*mapped == vport).then_some(*address))
    else {
        return;
    };
    if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(payload)) {
        if let Err(err) =
            rx_channel.send(RxMessage { address, packet, source: IfaceSource::None }).await
        {
            log::warn!("failed to enqueue RNodeMulti inbound packet iface={address}: {err}");
        }
    }
}

fn rnode_multi_tx_vports(
    message: &TxMessage,
    options: &RNodeMultiStreamOptions,
) -> Option<Vec<u8>> {
    match message.tx_type {
        TxMessageType::Direct(address) => options.vport_map.get(&address).copied().map(|v| vec![v]),
        TxMessageType::Broadcast(_) => {
            let vports = options
                .subinterfaces
                .iter()
                .filter(|sub| sub.outgoing)
                .map(|sub| sub.vport)
                .collect::<Vec<_>>();
            (!vports.is_empty()).then_some(vports)
        }
    }
}

async fn write_rnode_multi_data<IO>(stream: &mut IO, vport: u8, payload: &[u8]) -> bool
where
    IO: AsyncWrite + Unpin,
{
    let select = encode_command_frame(CMD_SEL_INT, &[vport]);
    let data = encode_command_frame(crate::kiss::CMD_DATA, payload);
    stream.write_all(&select).await.is_ok()
        && stream.write_all(&data).await.is_ok()
        && stream.flush().await.is_ok()
}

async fn write_rnode_multi_management_frame<IO>(stream: &mut IO, vport: u8, frame: &[u8]) -> bool
where
    IO: AsyncWrite + Unpin,
{
    let select = encode_command_frame(CMD_SEL_INT, &[vport]);
    stream.write_all(&select).await.is_ok()
        && stream.write_all(frame).await.is_ok()
        && stream.flush().await.is_ok()
}

async fn write_rnode_multi_id_beacon<IO>(
    stream: &mut IO,
    options: &RNodeMultiStreamOptions,
    beacon: &KissIdBeaconConfig,
) -> bool
where
    IO: AsyncWrite + Unpin,
{
    let payload = beacon.payload();
    let vports = options
        .subinterfaces
        .iter()
        .filter(|subinterface| subinterface.outgoing)
        .map(|subinterface| subinterface.vport)
        .collect::<Vec<_>>();
    if vports.is_empty() {
        return false;
    }
    for vport in vports {
        if !write_rnode_multi_data(stream, vport, &payload).await {
            return false;
        }
    }
    true
}

fn rnode_multi_initial_frames(subinterfaces: &[RNodeMultiSubInterfaceConfig]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for subinterface in subinterfaces {
        frames.push(encode_command_frame(CMD_SEL_INT, &[subinterface.vport]));
        frames.extend(subinterface.config.command_frames());
        frames.push(encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_ON]));
    }
    frames
}

fn rnode_multi_probe_frames() -> Vec<Vec<u8>> {
    vec![
        encode_command_frame(CMD_DETECT, &[DETECT_REQ]),
        encode_command_frame(CMD_FW_VERSION, &[0x00]),
        encode_command_frame(CMD_PLATFORM, &[0x00]),
        encode_command_frame(CMD_MCU, &[0x00]),
        encode_command_frame(CMD_INTERFACES, &[0x00]),
    ]
}

fn rnode_multi_shutdown_frames(subinterfaces: &[RNodeMultiSubInterfaceConfig]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for subinterface in subinterfaces {
        frames.push(encode_command_frame(CMD_SEL_INT, &[subinterface.vport]));
        frames.push(encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]));
        frames.push(encode_command_frame(CMD_LEAVE, &[0xff]));
    }
    frames
}

fn rnode_multi_external_framebuffer_frame(enable: bool) -> Vec<u8> {
    encode_command_frame(CMD_FB_EXT, &[u8::from(enable)])
}

fn rnode_multi_data_command_vport(command: u8) -> Option<u8> {
    match command {
        0x00 => Some(0),
        0x10 => Some(1),
        0x20 => Some(2),
        0x70 => Some(3),
        0x75 => Some(4),
        0x90 => Some(5),
        0xA0 => Some(6),
        0xB0 => Some(7),
        0xC0 => Some(8),
        0xD0 => Some(9),
        0xE0 => Some(10),
        0xF0 => Some(11),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll};

    use tokio::io::{duplex, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
    use tokio_util::sync::CancellationToken;

    use crate::buffer::OutputBuffer;
    use crate::hash::AddressHash;
    use crate::iface::lora::{
        BATTERY_STATE_CHARGING, CMD_BANDWIDTH, CMD_BLINK, CMD_CR, CMD_RANDOM, CMD_SF, CMD_STAT_BAT,
        CMD_STAT_CHTM, CMD_STAT_PHYPRM, CMD_STAT_RSSI, CMD_STAT_SNR,
    };
    use crate::iface::{IfaceRole, InterfaceManager, TxMessage, TxMessageType};
    use crate::kiss::decode_frames;
    use crate::packet::Packet;
    use crate::serde::Serialize;

    use super::*;

    fn packet_payload(packet: &Packet) -> Vec<u8> {
        let mut buffer = vec![0_u8; 512];
        let mut output = OutputBuffer::new(&mut buffer);
        packet.serialize(&mut output).expect("serialize packet");
        output.as_slice().to_vec()
    }

    #[derive(Default)]
    struct FailingReadStream {
        writes: Vec<u8>,
    }

    impl AsyncRead for FailingReadStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::other("synthetic rnode multi read failure")))
        }
    }

    impl AsyncWrite for FailingReadStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.writes.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    struct FailingWriteStream;

    impl AsyncRead for FailingWriteStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for FailingWriteStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::other("synthetic rnode multi data write failure")))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn test_options(child: AddressHash, vport: u8) -> RNodeMultiStreamOptions {
        test_options_with_management(child, vport).0
    }

    fn test_options_with_management(
        child: AddressHash,
        vport: u8,
    ) -> (RNodeMultiStreamOptions, RNodeMultiManagementHandle) {
        let mut vport_map = BTreeMap::new();
        vport_map.insert(child, vport);
        let (management_frame_tx, management_frame_rx) = rnode_multi_management_channel();
        let handle = RNodeMultiManagementHandle { tx: management_frame_tx };
        (
            RNodeMultiStreamOptions {
                parent_iface: AddressHash::new([0xAA; 16]),
                device: "test".to_string(),
                subinterfaces: vec![RNodeMultiSubInterfaceConfig {
                    name: "child".to_string(),
                    vport,
                    config: LoraConfig::us915_default(),
                    outgoing: true,
                }],
                runtime_status: Arc::new(Mutex::new(RNodeMultiRuntimeStatus::from_subinterfaces(
                    &[],
                ))),
                vport_map,
                mtu: DEFAULT_MTU,
                startup_probe: None,
                initial_frames: Vec::new(),
                shutdown_frames: Vec::new(),
                id_beacon: None,
                management_frame_rx,
            },
            handle,
        )
    }

    #[test]
    fn rnode_multi_uses_python_default_mtu_and_explicit_override() {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let adapter = RNodeMultiInterface::new("/dev/ttyACM0", manager.clone());

        assert_eq!(RNodeMultiInterface::mtu(), 508);
        assert_eq!(adapter.mtu_value(), 508);
        assert_eq!(adapter.configured_mtu(), 508);
        assert_eq!(
            RNodeMultiInterface::new_tcp("192.0.2.10:8001", manager).with_mtu(1024).mtu_value(),
            1024
        );
    }

    fn accepted_probe_status() -> RNodeMultiProbeStatus {
        let mut status = RNodeMultiProbeStatus::default();
        status.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
        status.accept_command(CMD_FW_VERSION, &[1, 74]).expect("firmware");
        status.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");
        status.accept_command(CMD_MCU, &[0x01]).expect("mcu");
        status.accept_command(CMD_INTERFACES, &[0, 0x11, 1, 0x21]).expect("interfaces");
        status
    }

    #[tokio::test]
    async fn rnode_multi_parent_shutdown_cleans_registered_virtual_vport_ifaces() {
        let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let parent_iface = {
            let mut manager = iface_manager.lock().await;
            manager.new_channel_with_role(8, IfaceRole::Multicast).address
        };
        let first_child = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
            .expect("first child iface");
        let second_child = iface_manager
            .lock()
            .await
            .register_virtual_iface(parent_iface, IfaceRole::VirtualUnicast)
            .expect("second child iface");
        let vport_map = BTreeMap::from([(first_child, 2), (second_child, 3)]);

        cleanup_rnode_multi_virtual_ifaces(&iface_manager, &vport_map).await;

        let manager = iface_manager.lock().await;
        assert_eq!(manager.role(&first_child), None);
        assert_eq!(manager.role(&second_child), None);
    }

    #[test]
    fn rnode_multi_probe_status_accepts_interfaces_and_validates_firmware_1_74() {
        let status = accepted_probe_status();

        assert_eq!(status.interfaces.get(&0), Some(&RNodeMultiInterfaceType::Sx126x));
        assert_eq!(status.interfaces.get(&1), Some(&RNodeMultiInterfaceType::Sx128x));
        status.validate_startup_probe(&[0, 1]).expect("valid RNodeMulti probe");
    }

    #[test]
    fn rnode_multi_probe_status_uses_reported_vports_for_sparse_interfaces() {
        let mut status = RNodeMultiProbeStatus::default();
        status.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
        status.accept_command(CMD_FW_VERSION, &[1, 74]).expect("firmware");
        status.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");
        status.accept_command(CMD_MCU, &[0x01]).expect("mcu");
        status.accept_command(CMD_INTERFACES, &[3, 0x21, 1, 0x11]).expect("interfaces");

        assert_eq!(status.interfaces.get(&1), Some(&RNodeMultiInterfaceType::Sx126x));
        assert_eq!(status.interfaces.get(&3), Some(&RNodeMultiInterfaceType::Sx128x));
        status.validate_startup_probe(&[1, 3]).expect("sparse vports are valid");
        let err = status.validate_startup_probe(&[0]).expect_err("unreported vport rejected");
        assert!(err.contains("configured vport 0"));
    }

    #[test]
    fn rnode_multi_probe_status_rejects_duplicate_reported_vports() {
        let mut status = accepted_probe_status();
        let err = status
            .accept_command(CMD_INTERFACES, &[2, 0x11, 2, 0x21])
            .expect_err("duplicate vport rejected");

        assert!(err.contains("repeated vport 2"));
    }

    #[test]
    fn rnode_multi_probe_status_rejects_single_rnode_1_52_firmware() {
        let mut status = accepted_probe_status();
        status.firmware_version = Some((1, 52));

        let err = status.validate_startup_probe(&[0]).expect_err("old firmware rejected");
        assert!(err.contains("below required 1.74"));
    }

    #[test]
    fn rnode_multi_probe_status_rejects_missing_configured_vport() {
        let status = accepted_probe_status();

        let err = status.validate_startup_probe(&[2]).expect_err("missing vport rejected");
        assert!(err.contains("configured vport 2"));
    }

    #[test]
    fn rnode_multi_probe_status_detects_display_capable_platforms() {
        let mut status = accepted_probe_status();

        status.platform = Some(PLATFORM_ESP32);
        assert!(status.has_display());
        status.platform = Some(PLATFORM_NRF52);
        assert!(status.has_display());
        status.platform = Some(crate::iface::lora::PLATFORM_AVR);
        assert!(!status.has_display());
    }

    #[test]
    fn rnode_multi_shutdown_frames_use_python_leave_payload() {
        let frames = rnode_multi_shutdown_frames(
            &test_options(AddressHash::new([0x05; 16]), 4).subinterfaces,
        );
        let decoded = decode_frames(&frames.concat(), 512).expect("decode shutdown frames");

        assert_eq!(
            decoded,
            vec![
                KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![4])),
                KissFrame::Command(KissCommand::Unknown(CMD_RADIO_STATE, vec![RADIO_STATE_OFF])),
                KissFrame::Command(KissCommand::Unknown(CMD_LEAVE, vec![0xff])),
            ]
        );
    }

    #[test]
    fn rnode_multi_runtime_status_routes_radio_commands_to_selected_vport() {
        let mut status = RNodeMultiRuntimeStatus::from_subinterfaces(
            &test_options(AddressHash::new([0x04; 16]), 2).subinterfaces,
        );

        assert_eq!(status.selected_vport, 2);
        status.accept_command(CMD_SEL_INT, &[2]).expect("select vport 2");
        status
            .accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
            .expect("record vport 2 bandwidth");
        status.accept_command(CMD_SF, &[9]).expect("record vport 2 sf");
        status.accept_command(CMD_CR, &[5]).expect("record vport 2 coding rate");
        status.accept_command(CMD_STAT_RSSI, &[200]).expect("record vport 2 rssi");
        status.accept_command(CMD_STAT_SNR, &[12]).expect("record vport 2 snr");
        status
            .accept_command(CMD_STAT_BAT, &[BATTERY_STATE_CHARGING, 87])
            .expect("record vport 2 battery");
        status.accept_command(CMD_RANDOM, &[0x5a]).expect("record vport 2 random byte");
        status
            .accept_command(CMD_STAT_CHTM, &[0, 50, 0, 75, 1, 0, 1, 29])
            .expect("record vport 2 channel telemetry");
        status
            .accept_command(CMD_STAT_PHYPRM, &[0, 250, 0, 10, 0, 12, 1, 244, 0, 5])
            .expect("record vport 2 phy telemetry");
        status.accept_command(CMD_SEL_INT, &[3]).expect("select vport 3");
        status.accept_command(CMD_SF, &[7]).expect("record vport 3 sf");

        let vport2 = status.status_for_vport(2).expect("vport 2 status");
        assert_eq!(vport2.bandwidth_hz, Some(125_000));
        assert_eq!(vport2.spreading_factor, Some(9));
        assert_eq!(vport2.coding_rate, Some(5));
        assert_eq!(vport2.rssi_dbm, Some(43));
        assert_eq!(vport2.snr_db, Some(3.0));
        assert_eq!(vport2.battery_state, Some(BATTERY_STATE_CHARGING));
        assert_eq!(vport2.battery_percent, Some(87));
        assert_eq!(vport2.random_byte, Some(0x5a));
        assert_eq!(vport2.airtime_short_percent, Some(0.5));
        assert_eq!(vport2.airtime_long_percent, Some(0.75));
        assert_eq!(vport2.channel_load_short_percent, Some(2.56));
        assert_eq!(vport2.channel_load_long_percent, Some(2.85));
        assert_eq!(vport2.symbol_time_ms, Some(0.25));
        assert_eq!(vport2.symbol_rate_baud, Some(10));
        assert_eq!(vport2.preamble_symbols, Some(12));
        assert_eq!(vport2.preamble_time_ms, Some(500));
        assert_eq!(vport2.csma_slot_time_ms, Some(5));
        assert_eq!(vport2.csma_difs_ms, None);
        let vport3 = status.status_for_vport(3).expect("vport 3 status");
        assert_eq!(vport3.spreading_factor, Some(7));
        assert_eq!(vport3.rssi_dbm, None);

        let snapshot = status.to_json();
        assert_eq!(snapshot["stream_state"].as_str(), Some("configured"));
        assert!(snapshot["last_error"].is_null());
        assert_eq!(snapshot["selected_vport"].as_u64(), Some(3));
        assert!(snapshot["startup_probe"].is_null());
        assert_eq!(snapshot["subinterfaces"]["2"]["bandwidth_hz"].as_u64(), Some(125_000));
        assert_eq!(snapshot["subinterfaces"]["2"]["spreading_factor"].as_u64(), Some(9));
        assert_eq!(snapshot["subinterfaces"]["2"]["coding_rate"].as_u64(), Some(5));
        assert_eq!(snapshot["subinterfaces"]["2"]["rssi_dbm"].as_i64(), Some(43));
        assert_eq!(snapshot["subinterfaces"]["2"]["airtime_short_percent"].as_f64(), Some(0.5));
        assert_eq!(
            snapshot["subinterfaces"]["2"]["battery_state_label"].as_str(),
            Some("charging")
        );
        assert_eq!(snapshot["subinterfaces"]["2"]["battery_percent"].as_u64(), Some(87));
        assert_eq!(snapshot["subinterfaces"]["2"]["random_byte"].as_u64(), Some(0x5a));
        assert_eq!(
            snapshot["subinterfaces"]["2"]["reported_bitrate_bps"].as_f64(),
            Some(1757.8125)
        );
        assert_eq!(snapshot["subinterfaces"]["2"]["framebuffer_bytes"].as_u64(), Some(0));
        assert_eq!(snapshot["subinterfaces"]["2"]["display_bytes"].as_u64(), Some(0));
        assert_eq!(snapshot["subinterfaces"]["3"]["spreading_factor"].as_u64(), Some(7));
        assert!(snapshot["subinterfaces"]["3"]["rssi_dbm"].is_null());
    }

    #[tokio::test]
    async fn rnode_multi_stream_writes_initial_frames_after_successful_probe() {
        let child = AddressHash::new([0x03; 16]);
        let (stream, mut peer) = duplex(4096);
        let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let mut options = test_options(child, 1);
        let runtime_status = Arc::clone(&options.runtime_status);
        options.startup_probe = Some(RNodeMultiStartupProbe {
            frames: rnode_multi_probe_frames(),
            required_vports: vec![1],
            timeout: Duration::from_millis(500),
        });
        options.initial_frames = vec![encode_command_frame(CMD_SEL_INT, &[1])];
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            options,
            cancel.clone(),
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        let mut bytes = vec![0_u8; 256];
        let n = peer.read(&mut bytes).await.expect("read probe frames");
        let frames = decode_frames(&bytes[..n], 512).expect("decode probe frames");
        assert_eq!(
            frames,
            vec![
                KissFrame::Command(KissCommand::Unknown(CMD_DETECT, vec![DETECT_REQ])),
                KissFrame::Command(KissCommand::Unknown(CMD_FW_VERSION, vec![0])),
                KissFrame::Command(KissCommand::Unknown(CMD_PLATFORM, vec![0])),
                KissFrame::Command(KissCommand::Unknown(CMD_MCU, vec![0])),
                KissFrame::Command(KissCommand::Unknown(CMD_INTERFACES, vec![0])),
            ]
        );

        peer.write_all(&encode_command_frame(CMD_DETECT, &[DETECT_RESP]))
            .await
            .expect("write detect");
        peer.write_all(&encode_command_frame(CMD_FW_VERSION, &[1, 74]))
            .await
            .expect("write firmware");
        peer.write_all(&encode_command_frame(CMD_PLATFORM, &[0x80])).await.expect("write platform");
        peer.write_all(&encode_command_frame(CMD_MCU, &[0x01])).await.expect("write mcu");
        peer.write_all(&encode_command_frame(CMD_INTERFACES, &[0, 0x11, 1, 0x21]))
            .await
            .expect("write interfaces");

        let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
            .await
            .expect("initial frame timeout")
            .expect("read initial frame");
        cancel.cancel();
        task.await.expect("stream task");

        let frames = decode_frames(&bytes[..n], 512).expect("decode initial frame");
        assert_eq!(frames, vec![KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![1]))]);
        let snapshot =
            runtime_status.lock().expect("rnode multi runtime status mutex poisoned").to_json();
        assert_eq!(snapshot["startup_probe"]["detected"].as_bool(), Some(true));
        assert_eq!(snapshot["startup_probe"]["firmware_version"]["label"].as_str(), Some("1.74"));
        assert_eq!(snapshot["startup_probe"]["platform"].as_u64(), Some(0x80));
        assert_eq!(snapshot["startup_probe"]["mcu"].as_u64(), Some(0x01));
        assert_eq!(snapshot["startup_probe"]["interfaces"]["0"].as_str(), Some("SX126X"));
        assert_eq!(snapshot["startup_probe"]["interfaces"]["1"].as_str(), Some("SX128X"));
        assert_eq!(
            snapshot["startup_probe"]["interface_summary"].as_str(),
            Some("0:SX126X,1:SX128X")
        );
    }

    #[tokio::test]
    async fn rnode_multi_stream_disables_external_framebuffer_before_display_shutdown() {
        let child = AddressHash::new([0x06; 16]);
        let (stream, mut peer) = duplex(4096);
        let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let mut options = test_options(child, 1);
        options.startup_probe = Some(RNodeMultiStartupProbe {
            frames: rnode_multi_probe_frames(),
            required_vports: vec![1],
            timeout: Duration::from_millis(500),
        });
        options.shutdown_frames = rnode_multi_shutdown_frames(&options.subinterfaces);
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            options,
            cancel.clone(),
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        let mut bytes = vec![0_u8; 512];
        let _ = peer.read(&mut bytes).await.expect("read probe frames");
        peer.write_all(&encode_command_frame(CMD_DETECT, &[DETECT_RESP]))
            .await
            .expect("write detect");
        peer.write_all(&encode_command_frame(CMD_FW_VERSION, &[1, 74]))
            .await
            .expect("write firmware");
        peer.write_all(&encode_command_frame(CMD_PLATFORM, &[PLATFORM_ESP32]))
            .await
            .expect("write platform");
        peer.write_all(&encode_command_frame(CMD_MCU, &[0x01])).await.expect("write mcu");
        peer.write_all(&encode_command_frame(CMD_INTERFACES, &[1, 0x11]))
            .await
            .expect("write interfaces");

        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
        let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
            .await
            .expect("shutdown frame timeout")
            .expect("read shutdown frames");
        task.await.expect("stream task");

        let decoded = decode_frames(&bytes[..n], 512).expect("decode shutdown frames");
        assert_eq!(
            decoded,
            vec![
                KissFrame::Command(KissCommand::Unknown(CMD_FB_EXT, vec![0])),
                KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![1])),
                KissFrame::Command(KissCommand::Unknown(CMD_RADIO_STATE, vec![RADIO_STATE_OFF])),
                KissFrame::Command(KissCommand::Unknown(CMD_LEAVE, vec![0xff])),
            ]
        );
    }

    #[tokio::test]
    async fn rnode_multi_stream_probe_failure_preserves_partial_probe_metadata() {
        let child = AddressHash::new([0x07; 16]);
        let (stream, mut peer) = duplex(4096);
        let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let mut options = test_options(child, 1);
        let runtime_status = Arc::clone(&options.runtime_status);
        options.startup_probe = Some(RNodeMultiStartupProbe {
            frames: rnode_multi_probe_frames(),
            required_vports: vec![1],
            timeout: Duration::from_millis(25),
        });
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            options,
            cancel,
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        let mut bytes = vec![0_u8; 256];
        let _ = peer.read(&mut bytes).await.expect("read probe frames");
        peer.write_all(&encode_command_frame(CMD_DETECT, &[DETECT_RESP]))
            .await
            .expect("write detect");
        peer.write_all(&encode_command_frame(CMD_FW_VERSION, &[1, 52]))
            .await
            .expect("write old firmware");
        peer.write_all(&encode_command_frame(CMD_PLATFORM, &[0x80])).await.expect("write platform");
        peer.write_all(&encode_command_frame(CMD_MCU, &[0x01])).await.expect("write mcu");
        peer.write_all(&encode_command_frame(CMD_INTERFACES, &[1, 0x21]))
            .await
            .expect("write interfaces");

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("rnode multi probe failure timeout")
            .expect("rnode multi stream task");

        let snapshot =
            runtime_status.lock().expect("rnode multi runtime status mutex poisoned").to_json();
        assert_eq!(snapshot["stream_state"].as_str(), Some("probe_failed"));
        assert_eq!(
            snapshot["last_error"].as_str(),
            Some("rnode multi firmware version 1.52 is below required 1.74")
        );
        assert_eq!(snapshot["startup_probe"]["detected"].as_bool(), Some(true));
        assert_eq!(snapshot["startup_probe"]["firmware_version"]["label"].as_str(), Some("1.52"));
        assert_eq!(snapshot["startup_probe"]["platform"].as_u64(), Some(0x80));
        assert_eq!(snapshot["startup_probe"]["mcu"].as_u64(), Some(0x01));
        assert_eq!(snapshot["startup_probe"]["interfaces"]["1"].as_str(), Some("SX128X"));
        assert_eq!(snapshot["startup_probe"]["interface_summary"].as_str(), Some("1:SX128X"));
    }

    #[tokio::test]
    async fn rnode_multi_stream_cancel_marks_runtime_closed_without_hardware() {
        let child = AddressHash::new([0x08; 16]);
        let (stream, _peer) = duplex(4096);
        let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let options = test_options(child, 1);
        let runtime_status = options.runtime_status.clone();
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            options,
            cancel.clone(),
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let running = {
                    let status =
                        runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
                    status.stream_state == "running"
                };
                if running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime reached running");
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("rnode multi stream shutdown timeout")
            .expect("rnode multi stream task");

        let status = runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
        assert_eq!(status.stream_state, "closed");
        assert!(status.last_error.is_none());
    }

    #[tokio::test]
    async fn rnode_multi_stream_read_failure_preserves_failed_runtime_state() {
        let child = AddressHash::new([0x09; 16]);
        let stream = FailingReadStream::default();
        let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let options = test_options(child, 1);
        let runtime_status = options.runtime_status.clone();

        tokio::time::timeout(
            Duration::from_secs(1),
            run_rnode_multi_stream(
                stream,
                options,
                cancel,
                CancellationToken::new(),
                rx_tx,
                Arc::new(tokio::sync::Mutex::new(tx_rx)),
            ),
        )
        .await
        .expect("rnode multi stream exits after read failure");

        let status = runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
        assert_eq!(status.stream_state, "read_failed");
        assert_eq!(status.last_error.as_deref(), Some("synthetic rnode multi read failure"));
    }

    #[tokio::test]
    async fn rnode_multi_stream_data_write_failure_preserves_failed_runtime_state() {
        let child = AddressHash::new([0x0A; 16]);
        let (rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let options = test_options(child, 1);
        let runtime_status = options.runtime_status.clone();

        let task = tokio::spawn(run_rnode_multi_stream(
            FailingWriteStream,
            options,
            cancel,
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let running = {
                    let status =
                        runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
                    status.stream_state == "running"
                };
                if running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime reached running");

        tx_tx
            .send(TxMessage { tx_type: TxMessageType::Direct(child), packet: Packet::default() })
            .await
            .expect("queue tx");

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("rnode multi stream write failure timeout")
            .expect("rnode multi stream task");

        let status = runtime_status.lock().expect("rnode multi runtime status mutex poisoned");
        assert_eq!(status.stream_state, "write_failed");
        assert_eq!(status.last_error.as_deref(), Some("data frame write failed"));
    }

    #[tokio::test]
    async fn rnode_multi_stream_routes_direct_tx_to_selected_vport() {
        let child = AddressHash::new([0x01; 16]);
        let (stream, mut peer) = duplex(4096);
        let (_rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            test_options(child, 2),
            cancel.clone(),
            CancellationToken::new(),
            _rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        let packet = Packet::default();
        tx_tx
            .send(TxMessage { tx_type: TxMessageType::Direct(child), packet })
            .await
            .expect("queue tx");
        let mut bytes = vec![0_u8; 256];
        let n = peer.read(&mut bytes).await.expect("read mux frames");
        cancel.cancel();
        task.await.expect("stream task");

        let frames = decode_frames(&bytes[..n], 512).expect("decode frames");
        assert_eq!(frames[0], KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![2])));
        assert!(matches!(frames[1], KissFrame::Data(_)));
    }

    #[tokio::test]
    async fn rnode_multi_management_handle_writes_selected_vport_command_frame() {
        let child = AddressHash::new([0x12; 16]);
        let (stream, mut peer) = duplex(4096);
        let (_rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let (options, handle) = test_options_with_management(child, 2);
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            options,
            cancel.clone(),
            CancellationToken::new(),
            _rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        handle
            .dispatch_frame(2, LoraConfig::blink_frame(0x03))
            .await
            .expect("queue vport management frame");
        let mut bytes = vec![0_u8; 256];
        let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
            .await
            .expect("management frame timeout")
            .expect("read management frames");
        cancel.cancel();
        task.await.expect("stream task");

        let frames = decode_frames(&bytes[..n], 512).expect("decode management frames");
        assert_eq!(
            frames,
            vec![
                KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![2])),
                KissFrame::Command(KissCommand::Unknown(CMD_BLINK, vec![0x03])),
            ]
        );
    }

    #[tokio::test]
    async fn rnode_multi_stream_transmits_id_beacon_on_outgoing_subinterfaces_after_first_tx() {
        let child = AddressHash::new([0x11; 16]);
        let mut options = test_options(child, 2);
        options.subinterfaces.push(RNodeMultiSubInterfaceConfig {
            name: "child-two".to_string(),
            vport: 3,
            config: LoraConfig::us915_default(),
            outgoing: true,
        });
        options.subinterfaces.push(RNodeMultiSubInterfaceConfig {
            name: "receive-only".to_string(),
            vport: 4,
            config: LoraConfig::us915_default(),
            outgoing: false,
        });
        options.id_beacon = Some(KissIdBeaconConfig {
            callsign: b"MYCALL-0".to_vec(),
            interval: Duration::from_millis(20),
            min_payload_len: 0,
        });
        let (stream, mut peer) = duplex(4096);
        let (_rx_tx, rx_rx) = tokio::sync::mpsc::channel(4);
        drop(rx_rx);
        let (tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            options,
            cancel.clone(),
            CancellationToken::new(),
            _rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        tx_tx
            .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
            .await
            .expect("queue broadcast tx");
        let mut bytes = vec![0_u8; 512];
        let first_read = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
            .await
            .expect("first packet timeout")
            .expect("read first packet frames");
        assert!(
            decode_frames(&bytes[..first_read], 512)
                .expect("decode first packet")
                .iter()
                .any(|frame| matches!(frame, KissFrame::Data(payload) if payload != b"MYCALL-0")),
            "first RNodeMulti tx must carry the packet before any ID beacon"
        );

        let mut beacon_frames = Vec::new();
        for _ in 0..8 {
            let n = tokio::time::timeout(Duration::from_secs(1), peer.read(&mut bytes))
                .await
                .expect("beacon timeout")
                .expect("read beacon frames");
            beacon_frames.extend(decode_frames(&bytes[..n], 512).expect("decode beacon frames"));
            let beacon_count = beacon_frames
                .iter()
                .filter(|frame| matches!(frame, KissFrame::Data(payload) if payload == b"MYCALL-0"))
                .count();
            if beacon_count >= 2 {
                break;
            }
        }
        cancel.cancel();
        task.await.expect("stream task");

        assert!(
            beacon_frames.contains(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![2])))
        );
        assert!(
            beacon_frames.contains(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![3])))
        );
        assert!(!beacon_frames
            .contains(&KissFrame::Command(KissCommand::Unknown(CMD_SEL_INT, vec![4]))));
        assert_eq!(
            beacon_frames
                .iter()
                .filter(|frame| matches!(frame, KissFrame::Data(payload) if payload == b"MYCALL-0"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn rnode_multi_stream_routes_inbound_vport_data_to_child_iface() {
        let child = AddressHash::new([0x02; 16]);
        let (stream, mut peer) = duplex(4096);
        let (rx_tx, mut rx_rx) = tokio::sync::mpsc::channel(4);
        let (_tx_tx, tx_rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run_rnode_multi_stream(
            stream,
            test_options(child, 3),
            cancel.clone(),
            CancellationToken::new(),
            rx_tx,
            Arc::new(tokio::sync::Mutex::new(tx_rx)),
        ));

        let payload = packet_payload(&Packet::default());
        peer.write_all(&encode_command_frame(0x70, &payload)).await.expect("write inbound frame");
        let message = rx_rx.recv().await.expect("rx message");
        cancel.cancel();
        task.await.expect("stream task");

        assert_eq!(message.address, child);
    }
}
