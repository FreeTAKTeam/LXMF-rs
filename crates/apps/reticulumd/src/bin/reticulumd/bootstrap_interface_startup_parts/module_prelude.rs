use super::super::{
    mark_interface_runtime_fields, mark_interface_startup_status, strict_tcp_client_preflight,
    tcp_bind_addr, tcp_bind_addr_is_in_use, tcp_listener_bind_host,
    with_interface_runtime_metadata,
};

use super::{InterfaceStartupFailure, TcpServerSelection};

use crate::bridge_rnode_management::DaemonRNodeManagementHandle;

use crate::interface_hot_apply::{
    hot_apply_interface_seed_key, mark_tcp_server_record_runtime_status,
};

use crate::interfaces::{
    auto, ble, common::interface_label, i2p, kiss, lora, meshtastic, pipe, reticulum_ble,
    rnode_multi, serial, udp, vrn76_kiss_ble, weave,
};

use crate::interfaces::common::{
    apply_runtime_config as apply_interface_runtime_config, spawn_configured,
    spawn_configured_attachment, spawn_configured_with_handle,
};
use crate::Args;

use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};

use rns_rpc::InterfaceRecord;

use rns_transport::hash::AddressHash;

use rns_transport::iface::tcp_client::{TcpClient, TcpSocketTuning};

use rns_transport::iface::tcp_server::TcpServer;

use rns_transport::iface::udp::UdpInterface;

use rns_transport::iface::{IfaceRole, InterfaceMode};

use rns_transport::transport::Transport;

use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
pub(crate) struct InterfaceStartupBatch {
    pub(crate) configured_roots: Vec<(usize, AddressHash)>,
    pub(crate) startup_successes: usize,
    pub(crate) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_hot_apply_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) tunnel_synth_ifaces: Vec<AddressHash>,
    pub(super) auto_runtime_refreshes: Vec<AutoRuntimeRefresh>,
    pub(super) auto_runtime_shutdowns: Vec<AutoRuntimeShutdown>,
    pub(super) pipe_runtime_refreshes: Vec<PipeRuntimeRefresh>,
    pub(super) udp_runtime_refreshes: Vec<UdpRuntimeRefresh>,
    pub(super) serial_runtime_refreshes: Vec<SerialRuntimeRefresh>,
    pub(super) kiss_runtime_refreshes: Vec<KissRuntimeRefresh>,
    pub(super) ble_gatt_runtime_refreshes: Vec<BleGattRuntimeRefresh>,
    pub(super) reticulum_ble_runtime_refreshes: Vec<ReticulumBleRuntimeRefresh>,
    pub(super) i2p_runtime_refreshes: Vec<I2pRuntimeRefresh>,
    pub(super) meshtastic_runtime_refreshes: Vec<MeshtasticRuntimeRefresh>,
    pub(super) tcp_runtime_refreshes: Vec<TcpRuntimeRefresh>,
    pub(super) weave_runtime_refreshes: Vec<WeaveRuntimeRefresh>,
    pub(super) rnode_multi_runtime_refreshes: Vec<RNodeMultiRuntimeRefresh>,
    pub(super) lora_runtime_refreshes: Vec<LoraRuntimeRefresh>,
    #[cfg(feature = "vrn76-kiss-ble")]
    pub(super) vrn76_runtime_refreshes: Vec<Vrn76RuntimeRefresh>,
    pub(super) rnode_management_bindings: Vec<RNodeManagementBinding>,
    pub(super) weave_control_bindings: Vec<WeaveControlBinding>,
}

pub(crate) struct AutoRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: auto::AutoRuntimeStatusHandle,
}

pub(crate) struct AutoRuntimeShutdown {
    pub(crate) host_iface: AddressHash,
    pub(crate) runtime: rns_transport::iface::auto_runtime::AutoDiscoveryRuntime,
    #[cfg(test)]
    pub(crate) iface_manager: Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
}

impl AutoRuntimeShutdown {
    pub(crate) async fn quiesce(self) {
        self.runtime.stop().await;
    }

    #[cfg(test)]
    pub(crate) async fn stop(self) -> bool {
        let manager = self.iface_manager.clone();
        let existed = manager.lock().await.online(&self.host_iface).is_some();
        let mut native =
            crate::configured_interface_runtime::NativeInterfaceRuntime::new(self.host_iface, None);
        native.auto_shutdown = Some(self);
        match crate::interface_hot_apply::stop_owned_native(&manager, None, native).await {
            Ok(()) => existed,
            Err(error) => {
                log::warn!("[daemon] test Auto shutdown failed: {error}");
                false
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct PipeRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::pipe::PipeRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct UdpRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::udp::UdpRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct SerialRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::serial::SerialRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct KissRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) runtime_key: &'static str,
    pub(crate) status: rns_transport::iface::kiss::KissRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct BleGattRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: ble::BleRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct ReticulumBleRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: reticulum_ble::ReticulumBleRuntimeStatusHandle,
}

struct UdpStartupSinks<'a> {
    startup_failures: &'a mut Vec<InterfaceStartupFailure>,
    runtime_refreshes: &'a mut Vec<UdpRuntimeRefresh>,
    seeded_hot_apply_interfaces: &'a mut Vec<(String, InterfaceRecord, AddressHash)>,
}

#[derive(Clone)]
pub(crate) struct I2pRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::i2p::I2pRuntimeStatusHandle,
}

pub(crate) struct MeshtasticRuntimeRefresh {
    pub(crate) simulation: tokio::task::JoinHandle<()>,
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::meshtastic::MeshtasticRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct TcpRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: TcpRuntimeStatusSource,
}

#[derive(Clone)]
pub(crate) enum TcpRuntimeStatusSource {
    Stream(rns_transport::iface::tcp_client::TcpRuntimeStatusHandle),
    Listener(rns_transport::iface::tcp_server::TcpListenerRuntimeStatusHandle),
}

#[cfg(test)]
impl TcpRuntimeStatusSource {
    #[must_use]
    pub(crate) fn runtime_key(&self) -> &'static str {
        match self {
            Self::Stream(_) => "stream_status",
            Self::Listener(_) => "listener_status",
        }
    }

    #[must_use]
    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Stream(status) => status.to_json(),
            Self::Listener(status) => status.to_json(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct WeaveRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::weave::WeaveRuntimeStatusHandle,
    pub(crate) handle: rns_transport::iface::weave::WeaveManagementHandle,
}

#[derive(Clone)]
pub(crate) struct WeaveControlBinding {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) name: String,
    pub(crate) handle: rns_transport::iface::weave::WeaveManagementHandle,
}

#[derive(Clone)]
pub(crate) struct RNodeMultiRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::rnode_multi::RNodeMultiRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct LoraRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: LoraRuntimeStatusSource,
}

#[cfg(feature = "vrn76-kiss-ble")]
#[derive(Clone)]
pub(crate) struct Vrn76RuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::vrn76_kiss_ble::Vrn76KissBleStatusHandle,
}

#[derive(Clone)]
pub(crate) struct RNodeManagementBinding {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) name: String,
    pub(crate) handle: DaemonRNodeManagementHandle,
}

#[derive(Clone)]
pub(crate) enum LoraRuntimeStatusSource {
    Lora(rns_transport::iface::lora::LoraRuntimeStatusHandle),
    #[cfg(feature = "rnode-ble")]
    RnodeBle(rns_transport::iface::rnode_ble::RnodeBleRuntimeStatusHandle),
}

impl LoraRuntimeStatusSource {
    #[must_use]
    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Lora(status) => status.to_json(),
            #[cfg(feature = "rnode-ble")]
            Self::RnodeBle(status) => status.to_json(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn startup_configured_interfaces(
    args: &Args,
    config: &DaemonConfig,
    selected_tcp_server: &TcpServerSelection,
    transport: &Transport,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    server_iface: Option<&AddressHash>,
    configured_interfaces: &mut [InterfaceRecord],
    reticulum_storage_path: &std::path::Path,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
    transport_identity_hash: Option<[u8; 16]>,
) -> InterfaceStartupBatch {
    startup_configured_interfaces_with_auto_plan_builder(
        args,
        config,
        selected_tcp_server,
        transport,
        iface_manager,
        server_iface,
        configured_interfaces,
        reticulum_storage_path,
        shared_reconnect_events,
        transport_identity_hash,
        auto::build_native_startup_plan,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        apply_interface_runtime_config, build_tcp_client_adapter, mark_ble_spawn_success,
        startup_configured_interfaces, startup_i2p, startup_kiss, startup_kiss_tcp_client,
        startup_pipe, startup_reticulum_ble, startup_rnode_multi, startup_serial,
        startup_tcp_server_record, startup_udp, startup_weave, UdpStartupSinks,
    };
    use crate::bridge_rnode_management::DaemonRNodeManagementHandle;
    use crate::Args;
    use base64::Engine;
    use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
    use rns_rpc::InterfaceRecord;
    use rns_transport::hash::AddressHash;
    use rns_transport::identity_bridge::to_transport_private_identity;
    use rns_transport::iface::tcp_client::TcpSocketTuning;
    use rns_transport::iface::{IfaceRole, InterfaceMode, InterfaceSharedConfig};
    use rns_transport::transport::{Transport, TransportConfig};
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    include!("tests/builders_and_startup.rs");
    include!("tests/managed_carriers.rs");
    include!("tests/shared_instance_policy.rs");
}
