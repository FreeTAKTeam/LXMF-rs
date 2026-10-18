use alloc::string::String;
use std::collections::BTreeMap;
use std::net::{TcpStream as StdTcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};
use tokio_util::sync::CancellationToken;

use crate::hash::AddressHash;
use crate::iface::kiss::{KissIdBeaconConfig, KISS_READ_FRAME_TIMEOUT};
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, is_ifac_violation, record_ifac_violation, IfacRuntime,
    IfacState, MAX_IFAC_SIZE_BYTES,
};
use crate::kiss::{encode_command_frame, KissCommand, KissFrame, KissStreamDecoder};

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
