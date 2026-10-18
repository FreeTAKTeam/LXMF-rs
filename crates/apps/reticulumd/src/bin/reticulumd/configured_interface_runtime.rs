use crate::bootstrap::{AutoRuntimeShutdown, LoraRuntimeStatusSource};
use crate::bridge_rnode_management::DaemonRNodeManagementHandle;
use rns_transport::hash::AddressHash;
use rns_transport::iface;

#[derive(Clone)]
pub(crate) enum NativeRuntimeStatus {
    TcpClient(iface::tcp_client::TcpRuntimeStatusHandle),
    TcpListener(iface::tcp_server::TcpListenerRuntimeStatusHandle),
    Udp(iface::udp::UdpRuntimeStatusHandle),
    Pipe(iface::pipe::PipeRuntimeStatusHandle),
    Auto(crate::interfaces::auto::AutoRuntimeStatusHandle),
    Serial(iface::serial::SerialRuntimeStatusHandle),
    Kiss {
        key: &'static str,
        status: iface::kiss::KissRuntimeStatusHandle,
    },
    BleGatt(crate::interfaces::ble::BleRuntimeStatusHandle),
    ReticulumBle(crate::interfaces::reticulum_ble::ReticulumBleRuntimeStatusHandle),
    I2p(iface::i2p::I2pRuntimeStatusHandle),
    Meshtastic(iface::meshtastic::MeshtasticRuntimeStatusHandle),
    Weave(iface::weave::WeaveRuntimeStatusHandle),
    RNodeMulti(iface::rnode_multi::RNodeMultiRuntimeStatusHandle),
    Lora(LoraRuntimeStatusSource),
    #[cfg(feature = "vrn76-kiss-ble")]
    Vrn76(iface::vrn76_kiss_ble::Vrn76KissBleStatusHandle),
}

impl NativeRuntimeStatus {
    pub(crate) fn projection(&self) -> (&'static str, &'static str, serde_json::Value) {
        match self {
            Self::TcpClient(status) => ("tcp", "stream_status", status.to_json()),
            Self::TcpListener(status) => ("tcp", "listener_status", status.to_json()),
            Self::Udp(status) => ("udp", "status", status.to_json()),
            Self::Pipe(status) => ("pipe", "status", status.to_json()),
            Self::Auto(status) => ("auto", "carrier_runtime", status.to_json()),
            Self::Serial(status) => ("serial", "status", status.to_json()),
            Self::Kiss { key, status } => (key, "status", status.to_json()),
            Self::BleGatt(status) => ("ble_gatt", "status", status.to_json()),
            Self::ReticulumBle(status) => ("reticulum_ble", "status", status.to_json()),
            Self::I2p(status) => ("i2p", "tunnel_status", status.to_json()),
            Self::Meshtastic(status) => ("meshtastic", "status", status.to_json()),
            Self::Weave(status) => ("weave", "status", status.to_json()),
            Self::RNodeMulti(status) => ("rnode_multi", "radio_status", status.to_json()),
            Self::Lora(status) => ("lora", "rnode_status", status.to_json()),
            #[cfg(feature = "vrn76-kiss-ble")]
            Self::Vrn76(status) => ("vrn76", "status", status.to_json()),
        }
    }
}

/// One real native root and the unique effects that must retire with it.
pub(crate) struct NativeInterfaceRuntime {
    pub(crate) address: AddressHash,
    pub(crate) status: Option<NativeRuntimeStatus>,
    pub(crate) auto_shutdown: Option<AutoRuntimeShutdown>,
    pub(crate) auxiliary: Vec<tokio::task::JoinHandle<()>>,
    pub(crate) rnode: Vec<(String, DaemonRNodeManagementHandle)>,
    pub(crate) weave: Vec<(String, iface::weave::WeaveManagementHandle)>,
}

impl NativeInterfaceRuntime {
    pub(crate) fn new(address: AddressHash, status: Option<NativeRuntimeStatus>) -> Self {
        Self {
            address,
            status,
            auto_shutdown: None,
            auxiliary: Vec::new(),
            rnode: Vec::new(),
            weave: Vec::new(),
        }
    }
}

pub(crate) struct ConfiguredInterfaceStartup {
    pub(crate) index: usize,
    pub(crate) record: rns_rpc::InterfaceRecord,
    pub(crate) native: Option<NativeInterfaceRuntime>,
}
