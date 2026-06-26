use super::super::{
    mark_interface_runtime_fields, mark_interface_startup_status, strict_tcp_client_preflight,
    with_interface_runtime_metadata,
};

use super::{InterfaceStartupFailure, TcpServerSelection};

use crate::interface_hot_apply::tcp_interface_key;

use crate::interfaces::{
    auto, ble, common::interface_label, i2p, kiss, lora, pipe, rnode_multi, serial, udp,
    vrn76_kiss_ble, weave,
};

use crate::Args;

use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};

use rns_rpc::InterfaceRecord;

use rns_transport::hash::AddressHash;

use rns_transport::iface::tcp_client::{TcpClient, TcpSocketTuning};

use rns_transport::iface::udp::UdpInterface;

use rns_transport::iface::{IfaceRole, InterfaceMode};

use rns_transport::transport::Transport;

use std::sync::Arc;

pub(super) struct InterfaceStartupBatch {
    pub(super) startup_successes: usize,
    pub(super) startup_failures: Vec<InterfaceStartupFailure>,
    pub(super) seeded_tcp_interfaces: Vec<(String, InterfaceRecord, AddressHash)>,
    pub(super) tunnel_synth_ifaces: Vec<AddressHash>,
    pub(super) connected_to_shared_instance: bool,
    pub(super) i2p_runtime_refreshes: Vec<I2pRuntimeRefresh>,
    pub(super) weave_runtime_refreshes: Vec<WeaveRuntimeRefresh>,
    pub(super) rnode_multi_runtime_refreshes: Vec<RNodeMultiRuntimeRefresh>,
}

#[derive(Clone)]
pub(crate) struct I2pRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::i2p::I2pRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct WeaveRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::weave::WeaveRuntimeStatusHandle,
}

#[derive(Clone)]
pub(crate) struct RNodeMultiRuntimeRefresh {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: rns_transport::iface::rnode_multi::RNodeMultiRuntimeStatusHandle,
}

pub(super) async fn startup_configured_interfaces(
    args: &Args,
    config: &DaemonConfig,
    selected_tcp_server: &TcpServerSelection,
    transport: &Transport,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    server_iface: Option<&AddressHash>,
    configured_interfaces: &mut [InterfaceRecord],
    shared_reconnect_events: Option<tokio::sync::mpsc::UnboundedSender<AddressHash>>,
    transport_identity_hash: Option<[u8; 16]>,
) -> InterfaceStartupBatch {
    let mut startup_successes = 0usize;
    let mut startup_failures = Vec::new();
    let mut seeded_tcp_interfaces = Vec::new();
    let mut tunnel_synth_ifaces = Vec::new();
    let mut connected_to_shared_instance = false;
    let mut i2p_runtime_refreshes = Vec::new();
    let mut weave_runtime_refreshes = Vec::new();
    let mut rnode_multi_runtime_refreshes = Vec::new();

    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() {
            continue;
        }
        let label = interface_label(iface, index);
        match iface.kind.as_str() {
            "tcp_server" | "backbone" => {
                startup_tcp_server_record(
                    index,
                    iface,
                    &label,
                    selected_tcp_server,
                    server_iface,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                );
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
                } else {
                    startup_tcp_server_record(
                        index,
                        iface,
                        &label,
                        selected_tcp_server,
                        server_iface,
                        &mut configured_interfaces[index],
                        &mut startup_failures,
                    );
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
            "tcp_client" | "backbone_client" => {
                if let Some(client_iface) = startup_tcp_client(
                    args,
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                    &mut seeded_tcp_interfaces,
                )
                .await
                {
                    startup_successes += 1;
                    tunnel_synth_ifaces.push(client_iface);
                }
            }
            "udp" => {
                if startup_udp(
                    args,
                    iface,
                    &label,
                    transport,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "auto" => {
                if startup_auto(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
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
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "pipe" => {
                if startup_pipe(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
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
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "vrn76_kiss_ble" => {
                if startup_vrn76_kiss_ble(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
                )
                .await
                {
                    startup_successes += 1;
                }
            }
            "lora" => {
                if startup_lora(
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
                }
            }
            "rnode_multi" => {
                if let Some(refresh) = startup_rnode_multi(
                    iface,
                    &label,
                    iface_manager,
                    &mut configured_interfaces[index],
                    &mut startup_failures,
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
        seeded_tcp_interfaces,
        tunnel_synth_ifaces,
        connected_to_shared_instance,
        i2p_runtime_refreshes,
        weave_runtime_refreshes,
        rnode_multi_runtime_refreshes,
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
) {
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
        return;
    }

    if iface.port.is_none() {
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            format!("{} requires port for startup", iface.kind),
        );
        return;
    }
    let runtime_iface = server_iface.map(ToString::to_string);
    mark_interface_startup_status(record, "active", None, runtime_iface.as_deref());
}

enum LocalUnixStartup {
    Active,
    Attached(AddressHash),
    Failed,
}

#[cfg(unix)]
async fn startup_local_unix(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::UnboundedSender<AddressHash>>,
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
async fn startup_local_unix_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    endpoint: rns_transport::iface::local::LocalUnixEndpoint,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::UnboundedSender<AddressHash>>,
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
    _shared_reconnect_events: Option<tokio::sync::mpsc::UnboundedSender<AddressHash>>,
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
) -> bool {
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
            return false;
        }
    };

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
    true
}

async fn startup_i2p(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    transport_identity_hash: Option<[u8; 16]>,
) -> Option<I2pRuntimeRefresh> {
    let adapter = match i2p::build_adapter(iface, iface_manager.clone()) {
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
    let i2p_metadata = match i2p_runtime_metadata_json(args, iface, &adapter, peer_count).await {
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
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
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
    let subinterface_count = adapter.subinterfaces().len();
    let rnode_multi_metadata = rnode_multi_runtime_metadata_json(iface, adapter.subinterfaces());
    let runtime_status = adapter.runtime_status_handle();
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
    Some(WeaveRuntimeRefresh { runtime_iface: weave_iface, status: runtime_status })
}

fn weave_runtime_metadata_json(
    adapter: &rns_transport::iface::weave::WeaveInterface,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("status".to_string(), adapter.runtime_status_json());
    serde_json::Value::Object(metadata)
}

async fn startup_tcp_client(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    seeded_tcp_interfaces: &mut Vec<(String, InterfaceRecord, AddressHash)>,
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
    let adapter = build_tcp_client_adapter(endpoint, iface);
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
    if let Some(key) = tcp_interface_key(record) {
        seeded_tcp_interfaces.push((key, record.clone(), client_iface));
    }
    Some(client_iface)
}

fn build_tcp_client_adapter(endpoint: String, iface: &InterfaceConfig) -> TcpClient {
    let mut adapter = TcpClient::new(endpoint);
    if iface.kind == "backbone_client" {
        adapter = adapter
            .with_socket_tuning(TcpSocketTuning::backbone())
            .with_backbone_liveness();
    }
    if let Some(mtu) = iface.mtu {
        adapter.with_mtu(mtu)
    } else {
        adapter
    }
}

async fn startup_local_tcp_attach(
    args: &Args,
    iface: &InterfaceConfig,
    label: &str,
    selected_tcp_server: &TcpServerSelection,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    shared_reconnect_events: Option<tokio::sync::mpsc::UnboundedSender<AddressHash>>,
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
    let mut adapter = build_tcp_client_adapter(endpoint.to_string(), iface);
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

#[cfg(test)]
mod tests {
    use super::{
        build_tcp_client_adapter, startup_i2p, startup_rnode_multi, startup_udp, startup_weave,
    };
    use base64::Engine;
    use crate::Args;
    use reticulum_daemon::config::InterfaceConfig;
    use rns_rpc::InterfaceRecord;
    use rns_transport::hash::AddressHash;
    use rns_transport::identity_bridge::to_transport_private_identity;
    use rns_transport::iface::IfaceRole;
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

        let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface);

        assert_eq!(adapter.addr(), "rmap.world:4242");
        assert_eq!(adapter.mtu_value(), 4096);
        assert!(adapter.socket_tuning().is_empty());
        assert!(!adapter.hdlc_liveness_enabled());
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

        let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface);

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

        let started = startup_udp(
            &args,
            &iface,
            "auto-style-multicast",
            &transport,
            &manager,
            &mut record,
            &mut startup_failures,
        )
        .await;

        assert!(started);
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

        let started = startup_rnode_multi(
            iface,
            "rnode-multi",
            &manager,
            &mut record,
            &mut startup_failures,
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
