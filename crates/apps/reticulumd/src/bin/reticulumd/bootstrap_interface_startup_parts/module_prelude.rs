use super::super::{
    mark_interface_runtime_fields, mark_interface_startup_status, strict_tcp_client_preflight,
    tcp_bind_addr, tcp_bind_addr_is_in_use, tcp_listener_bind_host, with_interface_runtime_metadata,
};

use super::{InterfaceStartupFailure, TcpServerSelection};

use crate::bridge_rnode_management::DaemonRNodeManagementHandle;

use crate::interface_hot_apply::hot_apply_interface_seed_key;

use crate::interfaces::{
    auto, ble, common::interface_label, i2p, kiss, lora, pipe, reticulum_ble, rnode_multi, serial, udp,
    vrn76_kiss_ble, weave,
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

pub(super) struct InterfaceStartupBatch {
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_hot_apply_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) tunnel_synth_ifaces: Vec<AddressHash>,
    pub(super) connected_to_shared_instance: bool,
    pub(super) auto_runtime_refreshes: Vec<AutoRuntimeRefresh>,
    pub(super) pipe_runtime_refreshes: Vec<PipeRuntimeRefresh>,
    pub(super) udp_runtime_refreshes: Vec<UdpRuntimeRefresh>,
    pub(super) serial_runtime_refreshes: Vec<SerialRuntimeRefresh>,
    pub(super) kiss_runtime_refreshes: Vec<KissRuntimeRefresh>,
    pub(super) ble_gatt_runtime_refreshes: Vec<BleGattRuntimeRefresh>,
    pub(super) reticulum_ble_runtime_refreshes: Vec<ReticulumBleRuntimeRefresh>,
    pub(super) i2p_runtime_refreshes: Vec<I2pRuntimeRefresh>,
    pub(super) tcp_runtime_refreshes: Vec<TcpRuntimeRefresh>,
    pub(super) weave_runtime_refreshes: Vec<WeaveRuntimeRefresh>,
    pub(super) rnode_multi_runtime_refreshes: Vec<RNodeMultiRuntimeRefresh>,
    pub(super) lora_runtime_refreshes: Vec<LoraRuntimeRefresh>,
    #[cfg(feature = "vrn76-kiss-ble")]
    pub(super) vrn76_runtime_refreshes: Vec<Vrn76RuntimeRefresh>,
    pub(super) rnode_management_bindings: Vec<RNodeManagementBinding>,
    pub(super) weave_control_bindings: Vec<WeaveControlBinding>,
}

#[derive(Clone)]
pub(crate) struct AutoRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: auto::AutoRuntimeStatusHandle,
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
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut seeded_hot_apply_interfaces = Vec::new();
    let mut tunnel_synth_ifaces = Vec::new();
    let mut connected_to_shared_instance = false;
    let mut auto_runtime_refreshes = Vec::new();
    let mut pipe_runtime_refreshes = Vec::new();
    let mut udp_runtime_refreshes = Vec::new();
    let mut serial_runtime_refreshes = Vec::new();
    let mut kiss_runtime_refreshes = Vec::new();
    let mut ble_gatt_runtime_refreshes = Vec::new();
    let mut reticulum_ble_runtime_refreshes = Vec::new();
    let mut i2p_runtime_refreshes = Vec::new();
    let mut tcp_runtime_refreshes = Vec::new();
    let mut weave_runtime_refreshes = Vec::new();
    let mut rnode_multi_runtime_refreshes = Vec::new();
    let mut lora_runtime_refreshes = Vec::new();
    #[cfg(feature = "vrn76-kiss-ble")]
    let mut vrn76_runtime_refreshes = Vec::new();
    let mut rnode_management_bindings = Vec::new();
    let mut weave_control_bindings = Vec::new();

    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() {
            continue;
        }
        let label = interface_label(iface, index);
        match iface.kind.as_str() {
            "tcp_server" | "backbone" => {
                if let Some(seed) = startup_tcp_server_record(
                    index,
                    iface,
                    &label,
                    selected_tcp_server,
                    server_iface,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                ) {
                    seeded_hot_apply_interfaces.push(seed);
                }
                if selected_tcp_server.selected_index == Some(index) {
                    if let Some(active_iface) = server_iface {
                        let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
                        let mut manager = iface_manager.lock().await;
                        manager.set_mode(*active_iface, mode);
                        apply_interface_runtime_config(&mut manager, *active_iface, iface);
                    }
                }
            }
            "local" if iface.shared_instance_type.as_deref() == Some("unix") => {
                match startup_local_unix(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    shared_reconnect_events.clone(),
                )
                .await
                {
                    LocalUnixStartup::Active => startup_successes += 1,
                    LocalUnixStartup::Attached(client_iface) => {
                        startup_successes += 1;
                        tunnel_synth_ifaces.push(client_iface);
                        connected_to_shared_instance = true;
                    }
                    LocalUnixStartup::Failed => {}
                }
            }
            "local_client" if iface.shared_instance_type.as_deref() == Some("unix") => {
                match startup_local_unix_client_attach(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    shared_reconnect_events.clone(),
                )
                .await
                {
                    LocalUnixStartup::Attached(client_iface) => {
                        startup_successes += 1;
                        tunnel_synth_ifaces.push(client_iface);
                        connected_to_shared_instance = true;
                    }
                    LocalUnixStartup::Active | LocalUnixStartup::Failed => {}
                }
            }
            "local" => {
                if selected_tcp_server.local_attach_index == Some(index) {
                    if let Some(client_iface) = startup_local_tcp_attach(
                        args,
                        iface,
                        &label,
                        selected_tcp_server,
                        iface_manager,
                        &mut configured_interfaces[index],
                        &mut startup_failures,
                        shared_reconnect_events.clone(),
                    )
                    .await
                    {
                        startup_successes += 1;
                        tunnel_synth_ifaces.push(client_iface);
                        connected_to_shared_instance = true;
                    }
                } else if iface.synthetic_shared_instance
                    && selected_tcp_server.selected_index != Some(index)
                {
                    match startup_synthetic_local_tcp_sidecar(
                        args,
                        iface,
                        &label,
                        iface_manager,
                        &mut configured_interfaces[index],
                        &mut startup_failures,
                        shared_reconnect_events.clone(),
                    )
                    .await
                    {
                        LocalTcpSidecarStartup::Active => startup_successes += 1,
                        LocalTcpSidecarStartup::Attached(client_iface) => {
                            startup_successes += 1;
                            tunnel_synth_ifaces.push(client_iface);
                            connected_to_shared_instance = true;
                        }
                        LocalTcpSidecarStartup::Failed => {}
                    }
                } else if let Some(seed) = startup_tcp_server_record(
                    index,
                    iface,
                    &label,
                    selected_tcp_server,
                    server_iface,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                ) {
                    seeded_hot_apply_interfaces.push(seed);
                }
                if selected_tcp_server.selected_index == Some(index) {
                    if let Some(active_iface) = server_iface {
                        let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
                        let mut manager = iface_manager.lock().await;
                        manager.set_mode(*active_iface, mode);
                        apply_interface_runtime_config(&mut manager, *active_iface, iface);
                    }
                }
            }
            "local_client" => {
                if let Some(client_iface) = startup_local_tcp_client_attach(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    shared_reconnect_events.clone(),
                )
                .await
                {
                    startup_successes += 1;
                    tunnel_synth_ifaces.push(client_iface);
                    connected_to_shared_instance = true;
                }
            }
            "tcp_client" | "backbone_client" => {
                if let Some(client_iface) = startup_tcp_client(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut seeded_hot_apply_interfaces,
                    &mut tcp_runtime_refreshes,
                    shared_reconnect_events.clone(),
                )
                .await
                {
                    startup_successes += 1;
                    tunnel_synth_ifaces.push(client_iface);
                }
            }
            "udp" => {
                let mut sinks = UdpStartupSinks {
                    startup_failures: &mut startup_failures,
                    runtime_refreshes: &mut udp_runtime_refreshes,
                    seeded_hot_apply_interfaces: &mut seeded_hot_apply_interfaces,
                };
                if startup_udp(
                    args,
                    iface,
                    &label,
                    transport,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut sinks,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "auto" => {
                if let Some(refresh) = startup_auto(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                    auto_runtime_refreshes.push(refresh);
                }
            }
            "serial" => {
                if startup_serial(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut serial_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "weave" => {
                if let Some(refresh) = startup_weave(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                    weave_control_bindings.push(WeaveControlBinding {
                        runtime_iface: refresh.runtime_iface,
                        name: label.clone(),
                        handle: refresh.handle.clone(),
                    });
                    weave_runtime_refreshes.push(refresh);
                }
            }
            "kiss" | "ax25_kiss" => {
                if startup_kiss(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut kiss_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "kiss_tcp_client" => {
                if startup_kiss_tcp_client(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut kiss_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "pipe" => {
                if let Some(refresh) = startup_pipe(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                    pipe_runtime_refreshes.push(refresh);
                }
            }
            "i2p" => {
                if let Some(refresh) = startup_i2p(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    reticulum_storage_path,
                    transport_identity_hash,
                )
                .await
                {
                    startup_successes += 1;
                    i2p_runtime_refreshes.push(refresh);
                }
            }
            "ble_gatt" => {
                if startup_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut ble_gatt_runtime_refreshes,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "reticulum_ble" => {
                if startup_reticulum_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut reticulum_ble_runtime_refreshes,
                    transport_identity_hash,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "vrn76_kiss_ble" => {
                let startup = startup_vrn76_kiss_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await;
                if startup.started {
                    startup_successes += 1;
                    #[cfg(feature = "vrn76-kiss-ble")]
                    if let Some(refresh) = startup.refresh {
                        vrn76_runtime_refreshes.push(refresh);
                    }
                }
            }
            "lora" => {
                let started = startup_lora(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await;
                if started.started {
                    startup_successes += 1;
                    if let Some(refresh) = started.refresh {
                        lora_runtime_refreshes.push(refresh);
                    }
                    if let Some(binding) = started.management_binding {
                        rnode_management_bindings.push(binding);
                    }
                }
            }
            "rnode_multi" => {
                if let Some(refresh) = startup_rnode_multi(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut rnode_management_bindings,
                )
                .await
                {
                    startup_successes += 1;
                    rnode_multi_runtime_refreshes.push(refresh);
                }
            }
            _ => record_startup_failure(
                &mut configured_interfaces[index],
                &mut startup_failures,
                label,
                iface.kind.clone(),
                format!("unsupported interface kind '{}'", iface.kind),
            ),
        }
    }

    InterfaceStartupBatch {
        startup_successes,
        startup_failures,
        seeded_hot_apply_interfaces,
        tunnel_synth_ifaces,
        connected_to_shared_instance,
        auto_runtime_refreshes,
        pipe_runtime_refreshes,
        udp_runtime_refreshes,
        serial_runtime_refreshes,
        kiss_runtime_refreshes,
        ble_gatt_runtime_refreshes,
        reticulum_ble_runtime_refreshes,
        i2p_runtime_refreshes,
        tcp_runtime_refreshes,
        weave_runtime_refreshes,
        rnode_multi_runtime_refreshes,
        lora_runtime_refreshes,
        #[cfg(feature = "vrn76-kiss-ble")]
        vrn76_runtime_refreshes,
        rnode_management_bindings,
        weave_control_bindings,
    }
}

fn startup_tcp_server_record(
    index: usize,
    iface: &InterfaceConfig,
    label: &str,
    selected_tcp_server: &TcpServerSelection,
    server_iface: Option<&AddressHash>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<(String, InterfaceRecord, AddressHash)> {
    let selected_for_startup = selected_tcp_server.selected_index == Some(index);
    if !selected_for_startup {
        mark_interface_startup_status(
            record,
            "shadowed_by_transport_override",
            Some(&format!(
                "{} ignored because --transport selected the active bind endpoint",
                iface.kind
            )),
            None,
        );
        let endpoint = iface
            .port
            .map(|port| {
                let host = iface
                    .host
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("0.0.0.0");
                format!("{}:{}", host, port)
            })
            .unwrap_or_else(|| "<missing-port>".to_string());
        log::warn!(
            "[daemon] {} startup skipped name={} endpoint={} selected={}",
            iface.kind,
            label,
            endpoint,
            selected_tcp_server.bind_addr.as_deref().unwrap_or("<none>")
        );
        return None;
    }

    if iface.port.is_none() {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            format!("{} requires port for startup", iface.kind),
        );
        return None;
    }
    let runtime_iface = server_iface.map(ToString::to_string);
    mark_interface_startup_status(record, "active", None, runtime_iface.as_deref());
    server_iface.and_then(|active_iface| {
        hot_apply_interface_seed_key(record).map(|key| (key, record.clone(), *active_iface))
    })
}

#[cfg_attr(not(unix), allow(dead_code))]
enum LocalUnixStartup {
    Active,
    Attached(AddressHash),
    Failed,
}

enum LocalTcpSidecarStartup {
    Active,
    Attached(AddressHash),
    Failed,
}

#[allow(clippy::too_many_arguments)]
async fn startup_synthetic_local_tcp_sidecar(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> LocalTcpSidecarStartup {
    let Some(port) = iface.port else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "synthetic local shared-instance requires port for startup".to_string(),
        );
        return LocalTcpSidecarStartup::Failed;
    };

    let host = match tcp_listener_bind_host(iface) {
        Ok(host) => host,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return LocalTcpSidecarStartup::Failed;
        }
    };
    let bind_addr = tcp_bind_addr(host.as_str(), port);
    if tcp_bind_addr_is_in_use(&bind_addr) {
        if let Some(client_iface) = startup_local_tcp_attach_endpoint(
            args,
            iface,
            label,
            bind_addr.as_str(),
            iface_manager,
            record,
            startup_failures,
            shared_reconnect_events,
        )
        .await
        {
            return LocalTcpSidecarStartup::Attached(client_iface);
        }
        return LocalTcpSidecarStartup::Failed;
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let mut adapter = TcpServer::new(bind_addr.clone(), iface_manager.clone())
        .with_client_mtu(iface.mtu.unwrap_or(TcpClient::DEFAULT_MTU))
        .with_prefer_ipv6(iface.prefer_ipv6.unwrap_or(false));
    if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
        adapter = adapter.with_client_forced_bitrate(bitrate_bps);
    }
    let local_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        TcpServer::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, local_iface, iface);
    }
    log::info!(
        "[daemon] synthetic local tcp sidecar enabled iface={} name={} bind={}",
        local_iface,
        label,
        bind_addr
    );
    let runtime_iface = local_iface.to_string();
    mark_interface_startup_status(record, "active", None, Some(runtime_iface.as_str()));
    LocalTcpSidecarStartup::Active
}

#[cfg(unix)]
async fn startup_local_unix(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> LocalUnixStartup {
    let Some(socket_path) = iface
        .socket_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local unix requires socket_path for startup".to_string(),
        );
        return LocalUnixStartup::Failed;
    };

    let endpoint = rns_transport::iface::local::LocalUnixEndpoint::from_config_value(socket_path);
    match rns_transport::iface::local::LocalUnixServer::preflight_bind_available(&endpoint).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            return startup_local_unix_attach(
                args,
                iface,
                label,
                endpoint,
                iface_manager,
                record,
                startup_failures,
                shared_reconnect_events,
            )
            .await;
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                format!("local unix bind preflight failed endpoint={} err={}", endpoint.label(), err),
            );
            return LocalUnixStartup::Failed;
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let adapter = rns_transport::iface::local::LocalUnixServer::new_endpoint(
        endpoint.clone(),
        iface_manager.clone(),
    )
    .with_client_mtu(iface.mtu.unwrap_or(TcpClient::DEFAULT_MTU));
    let adapter = if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
        adapter.with_client_forced_bitrate(bitrate_bps)
    } else {
        adapter
    };
    let local_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        rns_transport::iface::local::LocalUnixServer::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, local_iface, iface);
    }
    log::info!(
        "[daemon] local unix enabled iface={} name={} socket_path={}",
        local_iface,
        label,
        endpoint.label()
    );
    let runtime_iface = local_iface.to_string();
    mark_interface_startup_status(record, "active", None, Some(runtime_iface.as_str()));
    LocalUnixStartup::Active
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
async fn startup_local_unix_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    endpoint: rns_transport::iface::local::LocalUnixEndpoint,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> LocalUnixStartup {
    if args.strict_interface_startup {
        if let Err(err) = rns_transport::iface::local::preflight_unix_connect(&endpoint).await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return LocalUnixStartup::Failed;
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let mut adapter = rns_transport::iface::local::LocalUnixClient::new_connect(endpoint.clone())
        .with_mtu(iface.mtu.unwrap_or(TcpClient::DEFAULT_MTU));
    if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
        adapter = adapter.with_forced_bitrate(bitrate_bps);
    }
    if let Some(events) = shared_reconnect_events {
        adapter = adapter.with_reconnect_events(events);
    }
    let client_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        rns_transport::iface::local::LocalUnixClient::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, client_iface, iface);
    }
    log::info!(
        "[daemon] local unix attached iface={} name={} socket_path={}",
        client_iface,
        label,
        endpoint.label()
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "attached", None, Some(runtime_iface.as_str()));
    LocalUnixStartup::Attached(client_iface)
}

#[cfg(not(unix))]
async fn startup_local_unix(
    _args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    _iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    _shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> LocalUnixStartup {
    record_startup_failure(
        record,
        startup_failures,
        label.to_string(),
        iface.kind.clone(),
        "local unix shared_instance_type is only supported on unix platforms".to_string(),
    );
    LocalUnixStartup::Failed
}

async fn startup_pipe(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<PipeRuntimeRefresh> {
    let adapter = match pipe::build_adapter(iface) {
        Ok(adapter) => adapter,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    };

    let pipe_metadata = pipe_runtime_metadata_json(&adapter);
    let runtime_status = adapter.runtime_status_handle();
    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let pipe_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        rns_transport::iface::pipe::PipeInterface::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, pipe_iface, iface);
    }
    log::info!("[daemon] pipe enabled iface={} name={}", pipe_iface, label);
    let runtime_iface = pipe_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("pipe".to_string(), pipe_metadata);
    });
    Some(PipeRuntimeRefresh { runtime_iface: pipe_iface, status: runtime_status })
}

fn pipe_runtime_metadata_json(
    adapter: &rns_transport::iface::pipe::PipeInterface,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("status".to_string(), adapter.runtime_status_json());
    serde_json::Value::Object(metadata)
}

#[allow(clippy::too_many_arguments)]
async fn startup_i2p(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    reticulum_storage_path: &std::path::Path,
    transport_identity_hash: Option<[u8; 16]>,
) -> Option<I2pRuntimeRefresh> {
    let effective_iface = i2p_config_with_storage_default(iface, reticulum_storage_path);
    let adapter = match i2p::build_adapter(&effective_iface, iface_manager.clone()) {
        Ok(adapter) => adapter,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    }
    .with_transport_identity_hash(transport_identity_hash);

    if args.strict_interface_startup {
        if let Err(err) = adapter.preflight_sam().await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    }

    let peer_count = adapter.peers().len();
    let i2p_metadata =
        match i2p_runtime_metadata_json(args, &effective_iface, &adapter, peer_count).await {
        Ok(metadata) => metadata,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    };
    let runtime_status = adapter.runtime_status_handle();
    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let i2p_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        rns_transport::iface::i2p::I2pInterface::spawn,
        IfaceRole::Multicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, i2p_iface, iface);
    }
    log::info!(
        "[daemon] i2p enabled iface={} name={} sam={}:{} peers={}",
        i2p_iface,
        label,
        iface.sam_host.as_deref().unwrap_or("<unset>"),
        iface.sam_port.unwrap_or_default(),
        peer_count
    );
    let runtime_iface = i2p_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("i2p".to_string(), i2p_metadata);
    });
    Some(I2pRuntimeRefresh { runtime_iface: i2p_iface, status: runtime_status })
}

fn i2p_config_with_storage_default(
    iface: &InterfaceConfig,
    reticulum_storage_path: &std::path::Path,
) -> InterfaceConfig {
    let mut effective = iface.clone();
    if effective.state_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none()
    {
        effective.state_path = Some(reticulum_storage_path.to_string_lossy().to_string());
    }
    effective
}

async fn i2p_runtime_metadata_json(
    args: &Args,
    iface: &InterfaceConfig,
    adapter: &rns_transport::iface::i2p::I2pInterface,
    peer_count: usize,
) -> Result<serde_json::Value, String> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "sam_endpoint".to_string(),
        serde_json::Value::String(format!(
            "{}:{}",
            iface.sam_host.as_deref().unwrap_or("127.0.0.1"),
            iface.sam_port.unwrap_or(rns_transport::iface::i2p::I2pInterface::DEFAULT_SAM_PORT)
        )),
    );
    metadata.insert(
        "peer_count".to_string(),
        serde_json::Value::Number((peer_count as u64).into()),
    );
    metadata.insert(
        "connectable".to_string(),
        serde_json::Value::Bool(iface.connectable.unwrap_or(false)),
    );
    metadata.insert("tunnel_status".to_string(), adapter.runtime_status_json());
    if let Some(state_path) = iface.state_path.as_deref().filter(|value| !value.trim().is_empty())
    {
        let path = std::path::PathBuf::from(state_path);
        let transport_identity_hash = adapter.transport_identity_hash();
        let key_path = rns_transport::iface::i2p::i2p_private_key_path_with_identity(
            &path,
            adapter.name(),
            transport_identity_hash.as_ref(),
        );
        metadata.insert(
            "private_key_path".to_string(),
            serde_json::Value::String(key_path.to_string_lossy().to_string()),
        );
        match std::fs::read_to_string(key_path.as_path()) {
            Ok(private_key) => {
                metadata.insert("private_key_persisted".to_string(), serde_json::Value::Bool(true));
                if let Ok(endpoint) =
                    rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                {
                    metadata.insert(
                        "reachable_endpoint".to_string(),
                        serde_json::Value::String(endpoint),
                    );
                }
            }
            Err(_) => {
                metadata
                    .insert("private_key_persisted".to_string(), serde_json::Value::Bool(false));
            }
        }
        if adapter.connectable() && !metadata.contains_key("reachable_endpoint") {
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                rns_transport::iface::i2p::connectable_session_destination_with_identity(
                    adapter.sam_addr(),
                    adapter.name(),
                    Some(&path),
                    transport_identity_hash.as_ref(),
                ),
            )
            .await
            {
                Ok(Ok(private_key)) => {
                    metadata
                        .insert("private_key_persisted".to_string(), serde_json::Value::Bool(true));
                    match rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                    {
                        Ok(endpoint) => {
                            metadata.insert(
                                "reachable_endpoint".to_string(),
                                serde_json::Value::String(endpoint),
                            );
                        }
                        Err(err) => {
                            metadata.insert(
                                "reachable_endpoint_error".to_string(),
                                serde_json::Value::String(err),
                            );
                        }
                    }
                }
                Ok(Err(err)) if args.strict_interface_startup => {
                    return Err(format!(
                        "i2p connectable destination preparation failed sam={} err={}",
                        adapter.sam_addr(),
                        err
                    ));
                }
                Ok(Err(err)) => {
                    metadata.insert(
                        "destination_prepare_error".to_string(),
                        serde_json::Value::String(err.to_string()),
                    );
                }
                Err(_) if args.strict_interface_startup => {
                    return Err(format!(
                        "i2p connectable destination preparation timed out sam={}",
                        adapter.sam_addr()
                    ));
                }
                Err(_) => {
                    metadata.insert(
                        "destination_prepare_error".to_string(),
                        serde_json::Value::String("timed out".to_string()),
                    );
                }
            }
        }
    }
    Ok(serde_json::Value::Object(metadata))
}

async fn startup_rnode_multi(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    rnode_management_bindings: &mut Vec<RNodeManagementBinding>,
) -> Option<RNodeMultiRuntimeRefresh> {
    let adapter = match rnode_multi::build_adapter(iface, iface_manager.clone()) {
        Ok(adapter) => adapter,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    };
    if args.strict_interface_startup {
        if let Err(err) = adapter.preflight_open() {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    }
    let subinterfaces = adapter.subinterfaces().to_vec();
    let subinterface_count = subinterfaces.len();
    let rnode_multi_metadata = rnode_multi_runtime_metadata_json(iface, &subinterfaces);
    let runtime_status = adapter.runtime_status_handle();
    let management_handle = adapter.rnode_management_handle();
    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let rnode_multi_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        rns_transport::iface::rnode_multi::RNodeMultiInterface::spawn,
        IfaceRole::Multicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, rnode_multi_iface, iface);
    }
    log::info!(
        "[daemon] rnode_multi enabled iface={} name={} device={} baud_rate={} subinterfaces={}",
        rnode_multi_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default(),
        subinterface_count
    );
    let runtime_iface = rnode_multi_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("rnode_multi".to_string(), rnode_multi_metadata);
    });
    let mut allowed_vports = subinterfaces
        .iter()
        .map(|subinterface| subinterface.vport)
        .collect::<Vec<_>>();
    allowed_vports.sort_unstable();
    allowed_vports.dedup();
    rnode_management_bindings.push(RNodeManagementBinding {
        runtime_iface: rnode_multi_iface,
        name: label.to_string(),
        handle: DaemonRNodeManagementHandle::RNodeMulti {
            handle: management_handle,
            allowed_vports,
        },
    });
    Some(RNodeMultiRuntimeRefresh { runtime_iface: rnode_multi_iface, status: runtime_status })
}

fn rnode_multi_runtime_metadata_json(
    iface: &InterfaceConfig,
    subinterfaces: &[rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig],
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    if let Some(device) = iface.device.as_deref().filter(|value| !value.trim().is_empty()) {
        metadata.insert("device".to_string(), serde_json::Value::String(device.to_string()));
    }
    if let Some(baud_rate) = iface.baud_rate {
        metadata.insert("baud_rate".to_string(), serde_json::Value::Number(baud_rate.into()));
    }
    metadata.insert(
        "subinterface_count".to_string(),
        serde_json::Value::Number((subinterfaces.len() as u64).into()),
    );
    metadata.insert(
        "subinterfaces".to_string(),
        serde_json::Value::Array(
            subinterfaces
                .iter()
                .map(rnode_multi_subinterface_runtime_json)
                .collect(),
        ),
    );
    metadata.insert(
        "radio_status".to_string(),
        rns_transport::iface::rnode_multi::RNodeMultiRuntimeStatus::from_subinterfaces(
            subinterfaces,
        )
        .to_json(),
    );
    serde_json::Value::Object(metadata)
}

fn rnode_multi_subinterface_runtime_json(
    subinterface: &rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig,
) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    entry.insert("name".to_string(), serde_json::Value::String(subinterface.name.clone()));
    entry.insert(
        "vport".to_string(),
        serde_json::Value::Number(u64::from(subinterface.vport).into()),
    );
    entry.insert("outgoing".to_string(), serde_json::Value::Bool(subinterface.outgoing));

    let config = subinterface.config;
    entry.insert(
        "frequency_hz".to_string(),
        serde_json::Value::Number(config.frequency_hz.into()),
    );
    entry.insert(
        "bandwidth_hz".to_string(),
        serde_json::Value::Number(u64::from(config.bandwidth_hz).into()),
    );
    entry.insert(
        "spreading_factor".to_string(),
        serde_json::Value::Number(u64::from(config.spreading_factor).into()),
    );
    entry.insert(
        "coding_rate".to_string(),
        serde_json::Value::Number(u64::from(config.coding_rate).into()),
    );
    entry.insert(
        "tx_power_dbm".to_string(),
        serde_json::Value::Number(i64::from(config.tx_power_dbm).into()),
    );
    entry.insert(
        "max_payload_bytes".to_string(),
        serde_json::Value::Number(u64::from(config.max_payload_bytes).into()),
    );
    if let Some(value) = config.airtime_limit_short_hundredths {
        entry.insert(
            "airtime_limit_short_hundredths".to_string(),
            serde_json::Value::Number(u64::from(value).into()),
        );
    }
    if let Some(value) = config.airtime_limit_long_hundredths {
        entry.insert(
            "airtime_limit_long_hundredths".to_string(),
            serde_json::Value::Number(u64::from(value).into()),
        );
    }
    serde_json::Value::Object(entry)
}

async fn startup_weave(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<WeaveRuntimeRefresh> {
    let adapter = match weave::build_adapter(iface, iface_manager.clone()) {
        Ok(adapter) => adapter,
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    };

    if args.strict_interface_startup {
        if let Err(err) = adapter.preflight_open() {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    }

    let weave_metadata = weave_runtime_metadata_json(&adapter);
    let runtime_status = adapter.runtime_status_handle();
    let control_handle = adapter.weave_management_handle();
    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let weave_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        rns_transport::iface::weave::WeaveInterface::spawn,
        IfaceRole::Multicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, weave_iface, iface);
    }
    log::info!(
        "[daemon] weave enabled iface={} name={} device={} baud_rate={}",
        weave_iface,
        label,
        iface.device.as_deref().unwrap_or("<unset>"),
        iface.baud_rate.unwrap_or_default()
    );
    let runtime_iface = weave_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("weave".to_string(), weave_metadata);
    });
    Some(WeaveRuntimeRefresh {
        runtime_iface: weave_iface,
        status: runtime_status,
        handle: control_handle,
    })
}

fn weave_runtime_metadata_json(
    adapter: &rns_transport::iface::weave::WeaveInterface,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("status".to_string(), adapter.runtime_status_json());
    serde_json::Value::Object(metadata)
}

#[allow(clippy::too_many_arguments)]
async fn startup_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    seeded_hot_apply_interfaces: &mut Vec<(String, InterfaceRecord, AddressHash)>,
    tcp_runtime_refreshes: &mut Vec<TcpRuntimeRefresh>,
    stream_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> Option<AddressHash> {
    let (Some(host), Some(port)) = (iface.host.as_ref(), iface.port) else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            format!("{} requires host and port for startup", iface.kind),
        );
        return None;
    };

    let endpoint = format!("{}:{}", host, port);
    if args.strict_interface_startup {
        if let Err(err) = strict_tcp_client_preflight(endpoint.as_str()).await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let adapter = build_tcp_client_adapter(endpoint, iface, stream_reconnect_events);
    let runtime_status = adapter.runtime_status_handle();
    let client_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        TcpClient::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, client_iface, iface);
    }
    log::info!(
        "[daemon] {} enabled iface={} name={} host={} port={}",
        iface.kind,
        client_iface,
        label,
        host,
        port
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    tcp_runtime_refreshes.push(TcpRuntimeRefresh {
        runtime_iface: client_iface,
        status: TcpRuntimeStatusSource::Stream(runtime_status),
    });
    if let Some(key) = hot_apply_interface_seed_key(record) {
        seeded_hot_apply_interfaces.push((key, record.clone(), client_iface));
    }
    Some(client_iface)
}

fn build_tcp_client_adapter(
    endpoint: String,
    iface: &InterfaceConfig,
    stream_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> TcpClient {
    let mut adapter = TcpClient::new(endpoint);
    if iface.kind == "backbone_client" {
        adapter = adapter
            .with_socket_tuning(TcpSocketTuning::backbone())
            .with_backbone_liveness();
    } else if iface.kind == "tcp_client" && iface.i2p_tunneled == Some(true) {
        adapter = adapter.with_socket_tuning(TcpSocketTuning::i2p_tunneled());
    }
    if let Some(connect_timeout) = iface.connect_timeout {
        adapter = adapter.with_connect_timeout(Duration::from_secs(connect_timeout));
    }
    if iface.max_reconnect_tries.is_some() {
        adapter = adapter.with_max_reconnect_tries(iface.max_reconnect_tries);
    }
    if let Some(prefer_ipv6) = iface.prefer_ipv6 {
        adapter = adapter.with_prefer_ipv6(prefer_ipv6);
    }
    if let Some(mtu) = iface.mtu {
        adapter = adapter.with_mtu(mtu);
    }
    if matches!(iface.kind.as_str(), "local" | "local_client") {
        if let Some(bitrate_bps) = iface.force_shared_instance_bitrate {
            adapter = adapter.with_forced_bitrate(bitrate_bps);
        }
    }
    if matches!(iface.kind.as_str(), "tcp_client" | "backbone_client") {
        if let Some(events) = stream_reconnect_events {
            adapter = adapter.with_reconnect_events(events);
        }
    }
    adapter
}

#[allow(clippy::too_many_arguments)]
async fn startup_local_tcp_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    selected_tcp_server: &TcpServerSelection,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> Option<AddressHash> {
    let Some(endpoint) = selected_tcp_server.local_attach_addr.as_deref() else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local attach requires a selected shared-instance endpoint".to_string(),
        );
        return None;
    };

    startup_local_tcp_attach_endpoint(
        args,
        iface,
        label,
        endpoint,
        iface_manager,
        record,
        startup_failures,
        shared_reconnect_events,
    )
    .await
}

async fn startup_local_tcp_client_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> Option<AddressHash> {
    let (Some(host), Some(port)) = (iface.host.as_deref(), iface.port) else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local_client attach requires host and port for startup".to_string(),
        );
        return None;
    };
    let endpoint = format!("{}:{}", host, port);
    startup_local_tcp_attach_endpoint(
        args,
        iface,
        label,
        endpoint.as_str(),
        iface_manager,
        record,
        startup_failures,
        shared_reconnect_events,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn startup_local_tcp_attach_endpoint(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    endpoint: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> Option<AddressHash> {
    if args.strict_interface_startup {
        if let Err(err) = strict_tcp_client_preflight(endpoint).await {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            return None;
        }
    }

    let mode = iface.interface_mode().unwrap_or(InterfaceMode::Full);
    let mut adapter = build_tcp_client_adapter(endpoint.to_string(), iface, None);
    if let Some(events) = shared_reconnect_events {
        adapter = adapter.with_reconnect_events(events);
    }
    let client_iface = iface_manager.lock().await.spawn_as_with_mode(
        adapter,
        TcpClient::spawn,
        IfaceRole::Unicast,
        mode,
    );
    {
        let mut manager = iface_manager.lock().await;
        apply_interface_runtime_config(&mut manager, client_iface, iface);
    }
    log::info!(
        "[daemon] local attached iface={} name={} endpoint={}",
        client_iface,
        label,
        endpoint
    );
    let runtime_iface = client_iface.to_string();
    mark_interface_startup_status(record, "attached", None, Some(runtime_iface.as_str()));
    Some(client_iface)
}

#[cfg(unix)]
async fn startup_local_unix_client_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> LocalUnixStartup {
    let Some(socket_path) = iface
        .socket_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            "local_client unix requires socket_path for startup".to_string(),
        );
        return LocalUnixStartup::Failed;
    };
    let endpoint = rns_transport::iface::local::LocalUnixEndpoint::from_config_value(socket_path);
    startup_local_unix_attach(
        args,
        iface,
        label,
        endpoint,
        iface_manager,
        record,
        startup_failures,
        shared_reconnect_events,
    )
    .await
}

#[cfg(not(unix))]
async fn startup_local_unix_client_attach(
    _args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    _iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    _shared_reconnect_events: Option<tokio::sync::mpsc::Sender<AddressHash>>,
) -> LocalUnixStartup {
    record_startup_failure(
        record,
        startup_failures,
        label.to_string(),
        iface.kind.clone(),
        "local_client unix shared_instance_type is only supported on unix platforms".to_string(),
    );
    LocalUnixStartup::Failed
}

#[cfg(test)]
mod tests {
    use super::{
        apply_interface_runtime_config, build_tcp_client_adapter, mark_ble_spawn_success,
        startup_configured_interfaces, startup_i2p, startup_kiss, startup_kiss_tcp_client,
        startup_pipe, startup_reticulum_ble, startup_rnode_multi, startup_serial,
        startup_tcp_server_record, startup_udp, startup_weave, UdpStartupSinks,
    };
    use crate::Args;
    use crate::bridge_rnode_management::DaemonRNodeManagementHandle;
    use base64::Engine;
    use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
    use rns_rpc::InterfaceRecord;
    use rns_transport::hash::AddressHash;
    use rns_transport::iface::tcp_client::TcpSocketTuning;
    use rns_transport::identity_bridge::to_transport_private_identity;
    use rns_transport::iface::{IfaceRole, InterfaceMode, InterfaceSharedConfig};
    use rns_transport::transport::{Transport, TransportConfig};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn tcp_client_builder_uses_python_fixed_mtu() {
        let iface = InterfaceConfig {
            kind: "tcp_client".to_string(),
            enabled: Some(true),
            host: Some("rmap.world".to_string()),
            port: Some(4242),
            mtu: Some(4096),
            ..InterfaceConfig::default()
        };

        let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, None);

        assert_eq!(adapter.addr(), "rmap.world:4242");
        assert_eq!(adapter.mtu_value(), 4096);
        assert!(adapter.socket_tuning().is_empty());
        assert!(!adapter.hdlc_liveness_enabled());
        assert!(!adapter.reconnect_events_enabled());
    }

    #[tokio::test]
    async fn startup_configured_interfaces_marks_unknown_kind_as_failed() {
        let config = DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "FutureReticulumInterface", enabled = true, name = "future" }
]
"#,
        )
        .expect("unknown interface kinds should parse for runtime reporting");
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut records = config
            .interfaces
            .iter()
            .map(|iface| InterfaceRecord {
                kind: iface.kind.clone(),
                enabled: iface.enabled(),
                host: iface.host.clone(),
                port: iface.port,
                name: iface.name.clone(),
                settings: iface.settings_json(),
            })
            .collect::<Vec<_>>();

        let batch = startup_configured_interfaces(
            &test_args(),
            &config,
            &super::super::TcpServerSelection::default(),
            &transport,
            &manager,
            None,
            &mut records,
            std::path::Path::new("."),
            None,
            None,
        )
        .await;

        assert_eq!(batch.startup_successes, 0);
        assert_eq!(batch.startup_failures.len(), 1);
        assert_eq!(batch.startup_failures[0].kind, "FutureReticulumInterface");
        assert_eq!(
            batch.startup_failures[0].error,
            "unsupported interface kind 'FutureReticulumInterface'"
        );
        let runtime = records[0]
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .expect("runtime metadata");
        assert_eq!(runtime["startup_status"].as_str(), Some("failed"));
        assert_eq!(
            runtime["startup_error"].as_str(),
            Some("unsupported interface kind 'FutureReticulumInterface'")
        );
    }

    #[tokio::test]
    async fn startup_configured_interfaces_routes_python_style_aliases_to_real_branches() {
        let config = DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "udp-alias", listen_ip = "127.0.0.1", listen_port = 0, forward_ip = "127.0.0.1", forward_port = 4242 },
  { type = "SerialInterface", enabled = true, name = "serial-alias", port = "/dev/does-not-exist-serial-alias", speed = 9600 },
  { type = "KISSInterface", enabled = true, name = "kiss-alias", port = "/dev/does-not-exist-kiss-alias", speed = 1200 },
  { type = "TCPClientInterface", enabled = true, name = "tcp-client-alias", target_host = "127.0.0.1", target_port = 65535 },
  { type = "BackboneClientInterface", enabled = true, name = "backbone-client-alias", target_host = "127.0.0.1", target_port = 65535 },
  { type = "LocalServerInterface", enabled = true, name = "local-server-alias", listen_ip = "127.0.0.1", listen_port = 0 },
  { type = "Vrn76KissBluetoothInterface", enabled = true, name = "vrn76-alias", device_name_filter = "VR-N76" }
]
"#,
        )
        .expect("python-style aliases should parse");
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut records = config
            .interfaces
            .iter()
            .map(|iface| InterfaceRecord {
                kind: iface.kind.clone(),
                enabled: iface.enabled(),
                host: iface.host.clone(),
                port: iface.port,
                name: iface.name.clone(),
                settings: iface.settings_json(),
            })
            .collect::<Vec<_>>();

        let batch = startup_configured_interfaces(
            &test_args(),
            &config,
            &super::super::TcpServerSelection::default(),
            &transport,
            &manager,
            None,
            &mut records,
            std::path::Path::new("."),
            None,
            None,
        )
        .await;

        assert!(
            batch
                .startup_failures
                .iter()
                .all(|failure| !failure.error.contains("unsupported interface kind")),
            "aliases should route to real startup branches, failures={:?}",
            batch.startup_failures
        );
        assert!(records.iter().all(|record| {
            record
                .settings
                .as_ref()
                .and_then(|settings| settings.get("_runtime"))
                .and_then(|runtime| runtime.get("startup_error"))
                .and_then(serde_json::Value::as_str)
                .is_none_or(|error| !error.contains("unsupported interface kind"))
        }));
    }

    #[test]
    fn backbone_client_builder_enables_socket_tuning_and_liveness() {
        let iface = InterfaceConfig {
            kind: "backbone_client".to_string(),
            enabled: Some(true),
            host: Some("rmap.world".to_string()),
            port: Some(4242),
            mtu: Some(1_048_576),
            ..InterfaceConfig::default()
        };

        let (reconnect_tx, _reconnect_rx) = tokio::sync::mpsc::channel(32);
        let adapter =
            build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, Some(reconnect_tx));

        assert_eq!(adapter.addr(), "rmap.world:4242");
        assert_eq!(adapter.mtu_value(), 1_048_576);
        assert_eq!(adapter.socket_tuning().nodelay, Some(true));
        assert_eq!(adapter.socket_tuning().keepalive, Some(true));
        assert_eq!(
            adapter.socket_tuning().tcp_keepalive_idle,
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            adapter.socket_tuning().tcp_keepalive_interval,
            Some(Duration::from_secs(2))
        );
        assert_eq!(adapter.socket_tuning().tcp_keepalive_retries, Some(12));
        assert_eq!(
            adapter.socket_tuning().tcp_user_timeout,
            Some(Duration::from_secs(24))
        );
        assert!(adapter.hdlc_liveness_enabled());
        assert!(adapter.reconnect_events_enabled());
    }

    #[test]
    fn tcp_client_builder_applies_reticulum_reconnect_options() {
        let iface = InterfaceConfig {
            kind: "tcp_client".to_string(),
            enabled: Some(true),
            host: Some("rmap.world".to_string()),
            port: Some(4242),
            connect_timeout: Some(7),
            max_reconnect_tries: Some(3),
            prefer_ipv6: Some(true),
            ..InterfaceConfig::default()
        };

        let (reconnect_tx, _reconnect_rx) = tokio::sync::mpsc::channel(32);
        let adapter =
            build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, Some(reconnect_tx));

        assert_eq!(adapter.connect_timeout(), Duration::from_secs(7));
        assert_eq!(adapter.max_reconnect_tries(), Some(3));
        assert!(adapter.prefer_ipv6());
        assert!(adapter.reconnect_events_enabled());
    }

    #[test]
    fn local_client_builder_forces_shared_instance_bitrate() {
        let iface = InterfaceConfig {
            kind: "local_client".to_string(),
            enabled: Some(true),
            host: Some("127.0.0.1".to_string()),
            port: Some(37_428),
            bitrate: Some(1_000_000),
            force_shared_instance_bitrate: Some(1_000_000),
            ..InterfaceConfig::default()
        };

        let adapter = build_tcp_client_adapter("127.0.0.1:37428".to_string(), &iface, None);

        assert_eq!(adapter.forced_bitrate_bps(), Some(1_000_000));
        assert!(adapter.socket_tuning().is_empty());
        assert!(!adapter.hdlc_liveness_enabled());
        assert!(!adapter.reconnect_events_enabled());
    }

    #[test]
    fn tcp_client_builder_applies_i2p_tunneled_socket_profile() {
        let iface = InterfaceConfig {
            kind: "tcp_client".to_string(),
            enabled: Some(true),
            host: Some("rmap.world".to_string()),
            port: Some(4242),
            i2p_tunneled: Some(true),
            ..InterfaceConfig::default()
        };

        let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, None);

        assert_eq!(adapter.socket_tuning(), TcpSocketTuning::i2p_tunneled());
        assert!(!adapter.hdlc_liveness_enabled());
        assert!(!adapter.reconnect_events_enabled());
    }

    #[test]
    fn apply_runtime_config_records_reticulum_shared_options() {
        let mut manager = rns_transport::iface::InterfaceManager::new(16);
        let channel = manager.new_channel(16);
        let iface = InterfaceConfig {
            kind: "kiss".to_string(),
            enabled: Some(true),
            outgoing: Some(false),
            bitrate: Some(1200),
            announce_cap: Some(5),
            announce_rate_target: Some(120),
            announce_rate_grace: Some(2),
            announce_rate_penalty: Some(30),
            bootstrap_only: Some(true),
            ifac_size: Some(16),
            networkname: Some("field-net".to_string()),
            pass_phrase: Some("shared-secret".to_string()),
            ingress_control: Some(false),
            egress_control: Some(true),
            ic_burst_hold: Some(1.5),
            ec_pr_freq: Some(0.5),
            discoverable: Some(true),
            announce_interval: Some(360),
            discovery_name: Some("field node".to_string()),
            discovery_encrypt: Some(true),
            reachable_on: Some("lxmf://field".to_string()),
            publish_ifac: Some(true),
            latitude: Some(45.5),
            longitude: Some(-63.5),
            height: Some(42.0),
            discovery_frequency: Some(915_000_000),
            discovery_bandwidth: Some(125_000),
            discovery_modulation: Some(1),
            ..InterfaceConfig::default()
        };

        apply_interface_runtime_config(&mut manager, *channel.address(), &iface);

        assert_eq!(manager.outgoing(channel.address()), Some(false));
        assert_eq!(manager.announce_pacing(channel.address()), Some((1200, 5)));
        assert_eq!(
            manager.shared_config(channel.address()),
            Some(&InterfaceSharedConfig {
                announce_rate_target: Some(120),
                announce_rate_grace: Some(2),
                announce_rate_penalty: Some(30),
                bootstrap_only: Some(true),
                ifac_size: Some(16),
                network_name: Some("field-net".to_string()),
                passphrase: Some("shared-secret".to_string()),
                ingress_control: Some(false),
                egress_control: Some(true),
                ic_burst_hold: Some(1.5),
                ec_pr_freq: Some(0.5),
                discoverable: Some(true),
                announce_interval: Some(21_600),
                discovery_name: Some("field node".to_string()),
                discovery_encrypt: Some(true),
                reachable_on: Some("lxmf://field".to_string()),
                publish_ifac: Some(true),
                latitude: Some(45.5),
                longitude: Some(-63.5),
                height: Some(42.0),
                discovery_frequency: Some(915_000_000),
                discovery_bandwidth: Some(125_000),
                discovery_modulation: Some(1),
                ..InterfaceSharedConfig::default()
            })
        );
    }

    #[tokio::test]
    async fn udp_startup_tags_multicast_config_as_multicast_role() {
        let args = test_args();
        let iface = InterfaceConfig {
            kind: "udp".to_string(),
            enabled: Some(true),
            name: Some("auto-style-multicast".to_string()),
            host: Some("239.255.0.1".to_string()),
            port: Some(0),
            target_host: Some("239.255.0.1".to_string()),
            target_port: Some(4242),
            ..InterfaceConfig::default()
        };
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut udp_runtime_refreshes = Vec::new();
        let mut seeded_hot_apply_interfaces = Vec::new();
        let mut sinks = UdpStartupSinks {
            startup_failures: &mut startup_failures,
            runtime_refreshes: &mut udp_runtime_refreshes,
            seeded_hot_apply_interfaces: &mut seeded_hot_apply_interfaces,
        };

        let started = startup_udp(
            &args,
            &iface,
            "auto-style-multicast",
            &transport,
            &manager,
            &mut record,
            &mut sinks,
        )
        .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        assert_eq!(udp_runtime_refreshes.len(), 1);
        let runtime_iface = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(|iface| iface.as_str())
            .expect("runtime iface");
        let runtime_iface =
            AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
        let udp_status = &record
            .settings
            .as_ref()
            .expect("settings")["_runtime"]["udp"]["status"];
        assert_eq!(udp_status["link_state"].as_str(), Some("configured"));
        assert_eq!(udp_status["role"].as_str(), Some("multicast"));
        assert!(udp_status["bind_addr"].as_str().expect("bind").ends_with(":0"));
        assert_eq!(udp_status["forward_addr"].as_str(), Some("239.255.0.1:4242"));
        assert_eq!(udp_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
        assert!(seeded_hot_apply_interfaces.is_empty());
    }

    #[tokio::test]
    async fn udp_startup_seeds_unicast_config_for_hot_apply() {
        let args = test_args();
        let iface = InterfaceConfig {
            kind: "udp".to_string(),
            enabled: Some(true),
            name: Some("udp-loopback".to_string()),
            host: Some("127.0.0.1".to_string()),
            port: Some(0),
            target_host: Some("127.0.0.1".to_string()),
            target_port: Some(4242),
            ..InterfaceConfig::default()
        };
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut udp_runtime_refreshes = Vec::new();
        let mut seeded_hot_apply_interfaces = Vec::new();
        let mut sinks = UdpStartupSinks {
            startup_failures: &mut startup_failures,
            runtime_refreshes: &mut udp_runtime_refreshes,
            seeded_hot_apply_interfaces: &mut seeded_hot_apply_interfaces,
        };

        let started =
            startup_udp(&args, &iface, "udp-loopback", &transport, &manager, &mut record, &mut sinks)
                .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        assert_eq!(udp_runtime_refreshes.len(), 1);
        assert_eq!(seeded_hot_apply_interfaces.len(), 1);
        assert_eq!(seeded_hot_apply_interfaces[0].0, "udp-loopback");
        assert_eq!(seeded_hot_apply_interfaces[0].1.name.as_deref(), Some("udp-loopback"));
        assert_eq!(seeded_hot_apply_interfaces[0].1.kind, "udp");
        assert_eq!(seeded_hot_apply_interfaces[0].2, udp_runtime_refreshes[0].runtime_iface);
    }

    #[test]
    fn tcp_server_startup_seeds_selected_loopback_config_for_hot_apply() {
        let iface = InterfaceConfig {
            kind: "tcp_server".to_string(),
            enabled: Some(true),
            name: Some("tcp-loopback".to_string()),
            host: Some("127.0.0.1".to_string()),
            port: Some(4242),
            ..InterfaceConfig::default()
        };
        let selected_tcp_server = super::super::TcpServerSelection {
            bind_addr: Some("127.0.0.1:4242".to_string()),
            selected_index: Some(2),
            kind: "tcp_server".to_string(),
            ..super::super::TcpServerSelection::default()
        };
        let runtime_iface = AddressHash::new([0x42; 16]);
        let mut startup_failures = Vec::new();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };

        let seed = startup_tcp_server_record(
            2,
            &iface,
            "tcp-loopback",
            &selected_tcp_server,
            Some(&runtime_iface),
            &mut record,
            &mut startup_failures,
        )
        .expect("selected loopback TCP server should seed hot apply");

        assert!(startup_failures.is_empty());
        assert_eq!(seed.0, "tcp-loopback");
        assert_eq!(seed.1.name.as_deref(), Some("tcp-loopback"));
        assert_eq!(seed.1.kind, "tcp_server");
        assert_eq!(seed.2, runtime_iface);
        assert_eq!(
            record
                .settings
                .as_ref()
                .and_then(|settings| settings.get("_runtime"))
                .and_then(|runtime| runtime.get("startup_status"))
                .and_then(serde_json::Value::as_str),
            Some("active")
        );

        let mut shadowed_record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: iface.host.clone(),
            port: iface.port,
            name: Some("shadowed-tcp".to_string()),
            settings: iface.settings_json(),
        };
        let shadowed_seed = startup_tcp_server_record(
            3,
            &iface,
            "shadowed-tcp",
            &selected_tcp_server,
            Some(&runtime_iface),
            &mut shadowed_record,
            &mut startup_failures,
        );

        assert!(shadowed_seed.is_none());
        assert_eq!(
            shadowed_record
                .settings
                .as_ref()
                .and_then(|settings| settings.get("_runtime"))
                .and_then(|runtime| runtime.get("startup_status"))
                .and_then(serde_json::Value::as_str),
            Some("shadowed_by_transport_override")
        );
    }

    #[tokio::test]
    async fn serial_startup_embeds_configured_runtime_status_without_strict_preflight() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "SerialInterface", enabled = true, name = "serial-main", port = "/dev/does-not-exist-serial", speed = 19200, databits = 7, parity = "even", stopbits = 2, flow_control = "hardware", mtu = 1024 }
]
"#,
        )
        .expect("parse serial config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut serial_runtime_refreshes = Vec::new();

        let started = startup_serial(
            &args,
            iface,
            "serial-main",
            &manager,
            &mut record,
            &mut startup_failures,
            &mut serial_runtime_refreshes,
        )
        .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        assert_eq!(serial_runtime_refreshes.len(), 1);
        let runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .expect("runtime settings");
        assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
        let runtime_iface =
            runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
        let runtime_iface =
            AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
        let serial_status = &runtime["serial"]["status"];
        assert_eq!(serial_status["link_state"].as_str(), Some("configured"));
        assert_eq!(serial_status["device"].as_str(), Some("/dev/does-not-exist-serial"));
        assert_eq!(serial_status["baud_rate"].as_u64(), Some(19_200));
        assert_eq!(serial_status["data_bits"].as_u64(), Some(7));
        assert_eq!(serial_status["parity"].as_str(), Some("even"));
        assert_eq!(serial_status["stop_bits"].as_u64(), Some(2));
        assert_eq!(serial_status["flow_control"].as_str(), Some("hardware"));
        assert_eq!(serial_status["mtu"].as_u64(), Some(1024));
        assert_eq!(serial_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    }

    #[tokio::test]
    async fn ax25_kiss_startup_marks_spawned_unicast_without_strict_preflight() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "AX25KISSInterface", enabled = true, name = "ax25-main", port = "/dev/does-not-exist-ax25", speed = 1200, callsign = "N0CALL", ssid = 1 }
]
"#,
        )
        .expect("parse ax25 kiss config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut kiss_runtime_refreshes = Vec::new();

        let started = startup_kiss(
            &args,
            iface,
            "ax25-main",
            &manager,
            &mut record,
            &mut startup_failures,
            &mut kiss_runtime_refreshes,
        )
        .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        assert_eq!(kiss_runtime_refreshes.len(), 1);
        let runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .expect("runtime settings");
        assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
        let runtime_iface =
            runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
        let runtime_iface =
            AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
        let kiss_status = &runtime["kiss"]["status"];
        assert_eq!(kiss_status["link_state"].as_str(), Some("configured"));
        assert_eq!(kiss_status["bearer"].as_str(), Some("serial"));
        assert_eq!(kiss_status["device"].as_str(), Some("/dev/does-not-exist-ax25"));
        assert_eq!(kiss_status["baud_rate"].as_u64(), Some(1200));
        assert_eq!(kiss_status["ax25"].as_bool(), Some(true));
        assert_eq!(kiss_status["callsign"].as_str(), Some("N0CALL"));
        assert_eq!(kiss_status["ssid"].as_u64(), Some(1));
        assert_eq!(kiss_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
        assert_eq!(kiss_runtime_refreshes[0].runtime_iface, runtime_iface);
        assert_eq!(kiss_runtime_refreshes[0].runtime_key, "kiss");
    }

    #[tokio::test]
    async fn kiss_tcp_client_startup_marks_spawned_unicast_without_strict_preflight() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "kiss_tcp_client", enabled = true, name = "kiss-wifi", host = "127.0.0.1", port = 65535, flow_control = true }
]
"#,
        )
        .expect("parse kiss tcp client config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut kiss_runtime_refreshes = Vec::new();

        let started = startup_kiss_tcp_client(
            &args,
            iface,
            "kiss-wifi",
            &manager,
            &mut record,
            &mut startup_failures,
            &mut kiss_runtime_refreshes,
        )
        .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        assert_eq!(kiss_runtime_refreshes.len(), 1);
        let runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .expect("runtime settings");
        assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
        let runtime_iface =
            runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
        let runtime_iface =
            AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
        let kiss_status = &runtime["kiss_tcp"]["status"];
        assert_eq!(kiss_status["link_state"].as_str(), Some("configured"));
        assert_eq!(kiss_status["bearer"].as_str(), Some("tcp"));
        assert_eq!(kiss_status["endpoint"].as_str(), Some("127.0.0.1:65535"));
        assert_eq!(kiss_status["kiss_flow_control"].as_bool(), Some(true));
        assert_eq!(kiss_status["ax25"].as_bool(), Some(false));
        assert_eq!(kiss_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
        assert_eq!(kiss_runtime_refreshes[0].runtime_iface, runtime_iface);
        assert_eq!(kiss_runtime_refreshes[0].runtime_key, "kiss_tcp");
    }

    #[tokio::test]
    async fn ble_gatt_spawn_success_marks_running_unicast_without_hardware() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-main", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38", mode = "ap" }
]
"#,
        )
        .expect("parse ble gatt config");
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let ble_iface =
            *manager.lock().await.new_channel_with_role(8, IfaceRole::Unicast).address();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };

        mark_ble_spawn_success(iface, "ble-main", &manager, &mut record, ble_iface).await;

        let runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .expect("runtime settings");
        assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
        assert_eq!(runtime["runtime_status"].as_str(), Some("running"));
        assert_eq!(runtime["reconnect_attempts"].as_u64(), Some(0));
        let runtime_iface =
            runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
        let runtime_iface =
            AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
        let manager = manager.lock().await;
        assert_eq!(runtime_iface, ble_iface);
        let ble_status = &runtime["ble_gatt"]["status"];
        assert_eq!(ble_status["link_state"].as_str(), Some("configured"));
        assert_eq!(ble_status["peripheral_id"].as_str(), Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(
            ble_status["service_uuid"].as_str(),
            Some("12345678-1234-1234-1234-1234567890ab")
        );
        assert_eq!(ble_status["write_char_uuid"].as_str(), Some("2A37"));
        assert_eq!(ble_status["notify_char_uuid"].as_str(), Some("2A38"));
        assert_eq!(ble_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
        assert_eq!(manager.role(&runtime_iface), Some(IfaceRole::Unicast));
        assert_eq!(manager.mode(&runtime_iface), Some(InterfaceMode::AccessPoint));
    }

    #[tokio::test]
    async fn reticulum_ble_startup_marks_columba_runtime_status() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "BLEInterface", enabled = true, name = "columba-ble", mode = "ap" }
]
"#,
        )
        .expect("parse reticulum ble config");
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut local_identity = [0_u8; 16];
        local_identity.copy_from_slice(identity.address_hash().as_slice());
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut refreshes = Vec::new();

        let started = startup_reticulum_ble(
            iface,
            "columba-ble",
            &manager,
            &mut record,
            &mut startup_failures,
            &mut refreshes,
            Some(local_identity),
        )
        .await;

        assert!(started);
        assert!(startup_failures.is_empty());
        assert_eq!(refreshes.len(), 1);
        let runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .expect("runtime settings");
        assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
        assert_eq!(runtime["runtime_status"].as_str(), Some("degraded"));
        let runtime_iface =
            runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
        let runtime_iface =
            AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
        let manager = manager.lock().await;
        assert_eq!(manager.role(&runtime_iface), Some(IfaceRole::Unicast));
        assert_eq!(manager.mode(&runtime_iface), Some(InterfaceMode::AccessPoint));
        assert_eq!(refreshes[0].runtime_iface, runtime_iface);
        let ble_status = &runtime["reticulum_ble"]["status"];
        assert_eq!(ble_status["link_state"].as_str(), Some("degraded"));
        assert_eq!(ble_status["scan_state"].as_str(), Some("native_backend_pending"));
        assert_eq!(
            ble_status["advertising_state"].as_str(),
            Some("native_backend_pending")
        );
        assert_eq!(
            ble_status["service_uuid"].as_str(),
            Some("37145b00-442d-4a94-917f-8f42c5da28e3")
        );
        assert_eq!(
            ble_status["tx_char_uuid"].as_str(),
            Some("37145b00-442d-4a94-917f-8f42c5da28e4")
        );
        assert_eq!(
            ble_status["rx_char_uuid"].as_str(),
            Some("37145b00-442d-4a94-917f-8f42c5da28e5")
        );
        assert_eq!(ble_status["mtu"].as_u64(), Some(185));
        assert_eq!(ble_status["max_connections"].as_u64(), Some(7));
        assert_eq!(ble_status["enable_central"].as_bool(), Some(true));
        assert_eq!(ble_status["enable_peripheral"].as_bool(), Some(true));
        assert_eq!(ble_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    }

    #[tokio::test]
    async fn rnode_multi_startup_tags_parent_as_multicast_role() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "/dev/does-not-exist-rnode-multi", radio0 = { vport = 2, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }, radio1 = { vport = 3, frequency = 920000000, bandwidth = 125000, spreadingfactor = 10, codingrate = 5, txpower = 14 } }
]
"#,
        )
        .expect("parse rnode multi config");
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();
        let mut rnode_management_bindings = Vec::new();
        let args = test_args();

        let started = startup_rnode_multi(
            &args,
            iface,
            "rnode-multi",
            &manager,
            &mut record,
            &mut startup_failures,
            &mut rnode_management_bindings,
        )
        .await;

        let refresh = started.expect("rnode multi should start");
        assert!(startup_failures.is_empty());
        assert_eq!(rnode_management_bindings.len(), 1);
        assert_eq!(rnode_management_bindings[0].name, "rnode-multi");
        assert!(matches!(
            &rnode_management_bindings[0].handle,
            DaemonRNodeManagementHandle::RNodeMulti { allowed_vports, .. }
                if allowed_vports == &vec![2, 3]
        ));
        let runtime_iface = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(|iface| iface.as_str())
            .expect("runtime iface");
        let runtime_iface =
            AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
        assert_eq!(refresh.runtime_iface, runtime_iface);
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
        assert_eq!(rnode_management_bindings[0].runtime_iface, runtime_iface);
        let rnode_multi_runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("rnode_multi"))
            .expect("rnode multi runtime metadata");
        assert_eq!(
            rnode_multi_runtime["radio_status"]["selected_vport"].as_u64(),
            Some(2)
        );
        assert_eq!(
            rnode_multi_runtime["radio_status"]["stream_state"].as_str(),
            Some("configured")
        );
        assert!(rnode_multi_runtime["radio_status"]["last_error"].is_null());
        assert_eq!(
            rnode_multi_runtime["radio_status"]["vports"]
                .as_array()
                .expect("status vports")
                .iter()
                .filter_map(|value| value.as_u64())
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(
            rnode_multi_runtime["radio_status"]["subinterfaces"]["2"]["airtime_short_percent"]
                .as_f64(),
            Some(0.0)
        );
        assert!(
            rnode_multi_runtime["radio_status"]["subinterfaces"]["3"]["spreading_factor"].is_null()
        );
    }

    #[tokio::test]
    async fn pipe_startup_embeds_runtime_status() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "PipeInterface", enabled = true, name = "pipe-main", command = "cat", respawn_delay = 0.1 }
]
"#,
        )
        .expect("parse pipe config");
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started =
            startup_pipe(iface, "pipe-main", &manager, &mut record, &mut startup_failures).await;

        assert!(started.is_some());
        assert!(startup_failures.is_empty());
        let runtime_iface = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(|iface| iface.as_str())
            .expect("runtime iface");
        let runtime_iface =
            AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
        let pipe_status = &record
            .settings
            .as_ref()
            .expect("settings")["_runtime"]["pipe"]["status"];
        assert_eq!(pipe_status["command"].as_str(), Some("cat"));
        assert_eq!(pipe_status["process_state"].as_str(), Some("configured"));
        assert_eq!(pipe_status["pipe_is_open"].as_bool(), Some(false));
        assert_eq!(pipe_status["respawn_attempts"].as_u64(), Some(0));
        assert!(pipe_status["last_error"].is_null());
    }

    #[tokio::test]
    async fn weave_startup_tags_parent_as_multicast_role() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "WeaveInterface", enabled = true, name = "weave-main", port = "/dev/does-not-exist-weave" }
]
"#,
        )
        .expect("parse weave config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started = startup_weave(
            &args,
            iface,
            "weave-main",
            &manager,
            &mut record,
            &mut startup_failures,
        )
        .await;

        let refresh = started.expect("weave should start");
        assert!(startup_failures.is_empty());
        let runtime_iface = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(|iface| iface.as_str())
            .expect("runtime iface");
        let runtime_iface =
            AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
        assert_eq!(refresh.runtime_iface, runtime_iface);
        assert!(refresh
            .handle
            .try_set_remote_display(Some([0x10, 0x20, 0x30, 0x40]), true)
            .is_ok());
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
        let weave_runtime = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("weave"))
            .expect("weave runtime metadata");
        assert_eq!(
            weave_runtime["status"]["device"].as_str(),
            Some("/dev/does-not-exist-weave")
        );
        assert_eq!(weave_runtime["status"]["link_state"].as_str(), Some("configured"));
        assert_eq!(weave_runtime["status"]["endpoint_count"].as_u64(), Some(0));
        assert!(weave_runtime["status"]["display"].is_null());
        assert!(weave_runtime["status"]["device_stats"].is_null());
    }

    #[tokio::test]
    async fn i2p_startup_tags_parent_as_multicast_role() {
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "I2PInterface", enabled = true, name = "i2p-main", peers = ["exampledestination.b32.i2p"] }
]
"#,
        )
        .expect("parse i2p config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started = startup_i2p(
            &args,
            iface,
            "i2p-main",
            &manager,
            &mut record,
            &mut startup_failures,
            std::path::Path::new("."),
            None,
        )
        .await;

        assert!(started.is_some());
        assert!(startup_failures.is_empty());
        let runtime_iface = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("iface"))
            .and_then(|iface| iface.as_str())
            .expect("runtime iface");
        let runtime_iface =
            AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
        assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
        let tunnel_status = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("i2p"))
            .and_then(|i2p| i2p.get("tunnel_status"))
            .expect("i2p tunnel status metadata");
        assert_eq!(
            tunnel_status["sam_endpoint"].as_str(),
            Some("127.0.0.1:7656")
        );
        assert_eq!(tunnel_status["configured_peer_count"].as_u64(), Some(1));
        assert_eq!(tunnel_status["accept_state"].as_str(), Some("closed"));
        let peers = tunnel_status["peers"].as_array().expect("peer rows");
        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0]["peer"].as_str(),
            Some("exampledestination.b32.i2p")
        );
        assert_eq!(peers[0]["state"].as_str(), Some("configured"));
    }

    #[tokio::test]
    async fn i2p_startup_reports_persisted_reachable_endpoint_metadata() {
        let root = std::env::temp_dir().join(format!(
            "lxmfrs-i2p-startup-metadata-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(root.as_path());
        let private_key = fake_i2p_private_key();
        let identity_hash = [0x42_u8; 16];
        let key_path = rns_transport::iface::i2p::i2p_private_key_old_format_path(
            &root,
            "i2p-main",
        );
        std::fs::create_dir_all(key_path.parent().expect("key dir")).expect("key dir");
        std::fs::write(key_path.as_path(), private_key.as_bytes()).expect("write key");
        let expected_endpoint =
            rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                .expect("expected endpoint");

        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(&format!(
            r#"
interfaces = [
  {{ type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true, storagepath = "{}" }}
]
"#,
            root.to_string_lossy()
        ))
        .expect("parse i2p config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started = startup_i2p(
            &args,
            iface,
            "i2p-main",
            &manager,
            &mut record,
            &mut startup_failures,
            std::path::Path::new("."),
            Some(identity_hash),
        )
        .await;

        assert!(started.is_some());
        assert!(startup_failures.is_empty());
        let i2p = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("i2p"))
            .expect("i2p runtime metadata");
        assert_eq!(i2p.get("connectable").and_then(|value| value.as_bool()), Some(true));
        assert_eq!(
            i2p.get("private_key_persisted").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            i2p.get("reachable_endpoint").and_then(|value| value.as_str()),
            Some(expected_endpoint.as_str())
        );
        assert_eq!(
            i2p.get("private_key_path").and_then(|value| value.as_str()),
            Some(key_path.to_string_lossy().as_ref())
        );
        let _ = std::fs::remove_dir_all(root.as_path());
    }

    #[tokio::test]
    async fn i2p_startup_uses_daemon_storage_path_when_config_omits_storagepath() {
        let root = std::env::temp_dir().join(format!(
            "lxmfrs-i2p-default-storage-metadata-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(root.as_path());
        let private_key = fake_i2p_private_key();
        let identity_hash = [0x42_u8; 16];
        let key_path = rns_transport::iface::i2p::i2p_private_key_old_format_path(
            &root,
            "i2p-main",
        );
        std::fs::create_dir_all(key_path.parent().expect("key dir")).expect("key dir");
        std::fs::write(key_path.as_path(), private_key.as_bytes()).expect("write key");
        let expected_endpoint =
            rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                .expect("expected endpoint");

        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
            r#"
interfaces = [
  { type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true }
]
"#,
        )
        .expect("parse i2p config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started = startup_i2p(
            &args,
            iface,
            "i2p-main",
            &manager,
            &mut record,
            &mut startup_failures,
            root.as_path(),
            Some(identity_hash),
        )
        .await;

        assert!(started.is_some());
        assert!(startup_failures.is_empty());
        let i2p = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("i2p"))
            .expect("i2p runtime metadata");
        assert_eq!(
            i2p.get("private_key_persisted").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            i2p.get("reachable_endpoint").and_then(|value| value.as_str()),
            Some(expected_endpoint.as_str())
        );
        assert_eq!(
            i2p.get("private_key_path").and_then(|value| value.as_str()),
            Some(key_path.to_string_lossy().as_ref())
        );
        let _ = std::fs::remove_dir_all(root.as_path());
    }

    #[tokio::test]
    async fn i2p_startup_reports_generated_reachable_endpoint_metadata() {
        let private_key = fake_i2p_private_key();
        let expected_endpoint =
            rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
                .expect("expected endpoint");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
        let sam_addr = listener.local_addr().expect("local addr");
        let private_key_for_server = private_key.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept destination generation");
            let mut reader = BufReader::new(socket);
            let mut lines = Vec::new();
            let responses = [
                "HELLO REPLY RESULT=OK VERSION=3.3\n".to_string(),
                format!("DEST REPLY PUB=public-destination PRIV={private_key_for_server}\n"),
            ];
            for response in responses {
                let mut line = String::new();
                reader.read_line(&mut line).await.expect("read command");
                lines.push(line.trim_end().to_string());
                reader.get_mut().write_all(response.as_bytes()).await.expect("write response");
            }
            lines
        });

        let root = std::env::temp_dir().join(format!(
            "lxmfrs-i2p-startup-generated-metadata-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(root.as_path());
        let identity_hash = [0x42_u8; 16];
        let expected_key_path = rns_transport::iface::i2p::i2p_private_key_new_format_path(
            &root,
            "i2p-main",
            &identity_hash,
        );
        let cfg = reticulum_daemon::config::DaemonConfig::from_toml(&format!(
            r#"
interfaces = [
  {{ type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true, sam_ip = "{}", sam_port = {}, storagepath = "{}" }}
]
"#,
            sam_addr.ip(),
            sam_addr.port(),
            root.to_string_lossy()
        ))
        .expect("parse i2p config");
        let args = test_args();
        let iface = &cfg.interfaces[0];
        let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let transport_identity = to_transport_private_identity(&identity);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let manager = transport.iface_manager();
        let mut record = InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: true,
            host: None,
            port: None,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        };
        let mut startup_failures = Vec::new();

        let started = startup_i2p(
            &args,
            iface,
            "i2p-main",
            &manager,
            &mut record,
            &mut startup_failures,
            std::path::Path::new("."),
            Some(identity_hash),
        )
        .await;

        assert!(started.is_some());
        assert!(startup_failures.is_empty());
        let i2p = record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("i2p"))
            .expect("i2p runtime metadata");
        assert_eq!(
            i2p.get("private_key_persisted").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            i2p.get("reachable_endpoint").and_then(|value| value.as_str()),
            Some(expected_endpoint.as_str())
        );
        assert_eq!(
            i2p.get("private_key_path").and_then(|value| value.as_str()),
            Some(expected_key_path.to_string_lossy().as_ref())
        );
        assert_eq!(
            std::fs::read_to_string(expected_key_path).expect("stored key"),
            private_key
        );
        let lines = server.await.expect("server lines");
        assert_eq!(lines[0], "HELLO VERSION MIN=3.0 MAX=3.3");
        assert_eq!(lines[1], "DEST GENERATE SIGNATURE_TYPE=7");
        let _ = std::fs::remove_dir_all(root.as_path());
    }

    fn fake_i2p_private_key() -> String {
        let mut private = vec![0_u8; 500];
        for (index, byte) in private.iter_mut().enumerate() {
            *byte = index as u8;
        }
        private[385] = 0;
        private[386] = 3;
        let engine = base64::engine::GeneralPurpose::new(
            &base64::alphabet::Alphabet::new(
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-~",
            )
            .expect("alphabet"),
            base64::engine::general_purpose::PAD,
        );
        engine.encode(private)
    }

    fn test_args() -> Args {
        Args {
            rpc: None,
            db: PathBuf::from("reticulum.db"),
            config: None,
            identity: None,
            announce_interval_secs: 0,
            transport: Some("127.0.0.1:0".to_string()),
            strict_interface_startup: false,
            rpc_tls_cert: None,
            rpc_tls_key: None,
            rpc_tls_client_ca: None,
            rpc_token_issuer: None,
            rpc_token_audience: None,
            rpc_token_secret_env: None,
            rpc_token_jti_ttl_ms: 60_000,
            rpc_token_clock_skew_ms: 5_000,
            rpc_unix: None,
            #[cfg(feature = "zmq-pipeline-rpc")]
            zmq_rpc_command: None,
        }
    }
}
