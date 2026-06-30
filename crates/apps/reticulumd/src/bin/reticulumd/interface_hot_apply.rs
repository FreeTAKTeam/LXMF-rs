use rns_rpc::{InterfaceMutationBridge, InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::udp::{UdpInterface, UdpRuntimeStatusHandle};
use rns_transport::iface::{IfaceRole, InterfaceManager, InterfaceMode};
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use tokio::sync::mpsc::{channel, error::TrySendError, Receiver, Sender};

use crate::bootstrap::{
    mark_interface_runtime_fields, mark_interface_runtime_managed, mark_interface_startup_status,
};
use interface_hot_apply_parts::record_settings::interface_record_shared_config;
use interface_hot_apply_parts::record_settings::{setting_bool, setting_str, setting_u64};
#[cfg(test)]
use interface_hot_apply_parts::udp_runtime_refresh::refresh_hot_apply_udp_runtime_status_once;
use interface_hot_apply_parts::udp_runtime_refresh::{
    attach_hot_apply_udp_runtime_status, spawn_hot_apply_udp_runtime_status_refresher,
    HotApplyUdpRefresh,
};

#[path = "interface_hot_apply_parts.rs"]
mod interface_hot_apply_parts;

#[derive(Clone)]
pub(super) struct InterfaceHotApplyBridge {
    tx: Sender<InterfaceHotApplyCommand>,
    #[cfg(test)]
    udp_refreshes: Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
}

const INTERFACE_HOT_APPLY_QUEUE_CAPACITY: usize = 64;

impl InterfaceHotApplyBridge {
    #[cfg(test)]
    pub(super) fn spawn(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    ) -> Self {
        Self::spawn_inner(iface_manager, seeded, None, None)
    }

    #[cfg(test)]
    pub(super) fn spawn_with_daemon(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
    ) -> Self {
        Self::spawn_inner(iface_manager, seeded, None, Some(daemon))
    }

    pub(super) fn spawn_with_transport_and_daemon(
        transport: Arc<Transport>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
    ) -> Self {
        Self::spawn_inner(transport.iface_manager(), seeded, Some(transport), Some(daemon))
    }

    fn spawn_inner(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        transport: Option<Arc<Transport>>,
        daemon: Option<Weak<RpcDaemon>>,
    ) -> Self {
        let (tx, rx) = channel(INTERFACE_HOT_APPLY_QUEUE_CAPACITY);
        let udp_refreshes = Arc::new(StdMutex::new(HashMap::new()));
        if let Some(daemon) = daemon.clone() {
            spawn_hot_apply_udp_runtime_status_refresher(daemon, udp_refreshes.clone());
        }
        tokio::spawn(run_interface_mutation_worker(
            iface_manager,
            rx,
            seeded,
            transport.clone(),
            udp_refreshes.clone(),
            daemon,
        ));
        Self {
            tx,
            #[cfg(test)]
            udp_refreshes,
        }
    }
}

impl InterfaceMutationBridge for InterfaceHotApplyBridge {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        validate_hot_apply_uniqueness(&interfaces)?;
        let effective = interfaces
            .iter()
            .cloned()
            .map(|mut record| {
                if matches!(record.kind.as_str(), "tcp_client" | "udp") && record.enabled {
                    mark_interface_startup_status(&mut record, "spawned", None, None);
                    mark_interface_runtime_managed(&mut record, "daemon_transport");
                    mark_interface_runtime_fields(&mut record, "running", 0);
                    if record.kind == "udp" {
                        mark_udp_record_runtime_status(&mut record, None);
                    }
                }
                record
            })
            .collect::<Vec<_>>();
        self.tx.try_send(InterfaceHotApplyCommand::Apply { interfaces }).map_err(|error| {
            match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "interface mutation queue is full")
                }
                TrySendError::Closed(_) => io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "interface mutation worker is not running",
                ),
            }
        })?;
        Ok(effective)
    }
}

enum InterfaceHotApplyCommand {
    Apply { interfaces: Vec<InterfaceRecord> },
}

#[derive(Clone)]
struct ManagedHotApplyInterface {
    record: InterfaceRecord,
    address: AddressHash,
}

async fn run_interface_mutation_worker(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    mut rx: Receiver<InterfaceHotApplyCommand>,
    seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    transport: Option<Arc<Transport>>,
    udp_refreshes: Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
    daemon: Option<Weak<RpcDaemon>>,
) {
    let mut managed = seeded
        .into_iter()
        .map(|(key, record, address)| (key, ManagedHotApplyInterface { record, address }))
        .collect::<HashMap<_, _>>();

    while let Some(command) = rx.recv().await {
        match command {
            InterfaceHotApplyCommand::Apply { interfaces } => {
                apply_hot_apply_interface_records(
                    &iface_manager,
                    &mut managed,
                    interfaces,
                    transport.as_ref(),
                    &udp_refreshes,
                    daemon.as_ref(),
                )
                .await;
            }
        }
    }
}

async fn apply_hot_apply_interface_records(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<String, ManagedHotApplyInterface>,
    interfaces: Vec<InterfaceRecord>,
    transport: Option<&Arc<Transport>>,
    udp_refreshes: &Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
    daemon: Option<&Weak<RpcDaemon>>,
) {
    let desired = interfaces
        .into_iter()
        .filter_map(|record| {
            let key = hot_apply_interface_key(&record)?;
            Some((key, record))
        })
        .collect::<HashMap<_, _>>();

    let current_keys = managed.keys().cloned().collect::<Vec<_>>();
    for key in current_keys {
        let should_remove = match (managed.get(&key), desired.get(&key)) {
            (Some(current), Some(next)) => {
                !next.enabled || hot_apply_interface_record_changed(&current.record, next)
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        if should_remove {
            if let Some(current) = managed.remove(&key) {
                udp_refreshes.lock().expect("udp refresh mutex poisoned").remove(&key);
                stop_hot_apply_interface(iface_manager, transport, current.address).await;
            }
        }
    }

    for (key, record) in desired {
        if !record.enabled {
            continue;
        }
        if let Some(current) = managed.get_mut(&key) {
            let mut guard = iface_manager.lock().await;
            apply_record_runtime_config(&mut guard, current.address, &record);
            current.record = record;
            continue;
        }
        if let Some((address, udp_status)) =
            spawn_hot_apply_interface(iface_manager, transport, &record).await
        {
            if let Some(status) = udp_status {
                status.update(|status| {
                    status.iface = Some(address.to_string());
                });
                attach_hot_apply_udp_runtime_status(daemon, &record, address, &status);
                udp_refreshes.lock().expect("udp refresh mutex poisoned").insert(
                    key.clone(),
                    HotApplyUdpRefresh { record: record.clone(), runtime_iface: address, status },
                );
            }
            managed.insert(key, ManagedHotApplyInterface { record, address });
        }
    }
}

fn validate_hot_apply_uniqueness(interfaces: &[InterfaceRecord]) -> Result<(), io::Error> {
    let mut seen = std::collections::HashSet::new();
    let mut seen_udp_bind_addresses = std::collections::HashSet::new();
    for record in interfaces {
        if record.kind == "udp" {
            let (bind_addr, _) = udp_bind_and_forward_addr(record)?;
            if record.enabled && !seen_udp_bind_addresses.insert(bind_addr.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("duplicate udp bind address '{bind_addr}'"),
                ));
            }
        }
        let Some(key) = hot_apply_interface_key(record) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate hot-apply interface key '{key}'"),
            ));
        }
    }
    Ok(())
}

async fn spawn_hot_apply_interface(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    record: &InterfaceRecord,
) -> Option<(AddressHash, Option<UdpRuntimeStatusHandle>)> {
    let mode = interface_record_mode(record);
    let (address, udp_status) = match record.kind.as_str() {
        "tcp_client" => (
            iface_manager.lock().await.spawn_as_with_mode(
                TcpClient::new(tcp_endpoint(record)?),
                TcpClient::spawn,
                IfaceRole::Unicast,
                mode,
            ),
            None,
        ),
        "udp" => {
            let (bind_addr, forward_addr) = udp_bind_and_forward_addr(record).ok()?;
            let adapter = UdpInterface::new(bind_addr.clone(), forward_addr.clone());
            if adapter.is_multicast() {
                let transport = transport?;
                let (address, status) = transport
                    .add_multicast_udp_interface_with_status(bind_addr, forward_addr)
                    .await;
                (address, Some(status))
            } else {
                let status = adapter.runtime_status_handle();
                (
                    iface_manager.lock().await.spawn_as_with_mode(
                        adapter,
                        UdpInterface::spawn,
                        IfaceRole::Unicast,
                        mode,
                    ),
                    Some(status),
                )
            }
        }
        _ => return None,
    };
    {
        let mut guard = iface_manager.lock().await;
        apply_record_runtime_config(&mut guard, address, record);
    }
    Some((address, udp_status))
}

async fn stop_hot_apply_interface(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    address: AddressHash,
) {
    if let Some(transport) = transport {
        let _ = transport.stop_interface(address).await;
    } else {
        let mut guard = iface_manager.lock().await;
        let _ = guard.stop_interface(address);
    }
}

fn hot_apply_interface_key(record: &InterfaceRecord) -> Option<String> {
    match record.kind.as_str() {
        "tcp_client" => tcp_interface_key(record),
        "udp" => udp_interface_key(record),
        _ => None,
    }
}

pub(super) fn tcp_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "tcp_client" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    let host = record.host.as_ref()?.trim();
    let port = record.port?;
    Some(format!("{host}:{port}"))
}

pub(super) fn hot_apply_interface_seed_key(record: &InterfaceRecord) -> Option<String> {
    match record.kind.as_str() {
        "tcp_client" => tcp_interface_key(record),
        "udp" => {
            udp_bind_and_forward_addr(record).ok()?;
            udp_interface_key(record)
        }
        _ => None,
    }
}

fn udp_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "udp" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    let host = record.host.as_ref()?.trim();
    let port = record.port?;
    Some(format!("{host}:{port}"))
}

fn hot_apply_interface_record_changed(current: &InterfaceRecord, next: &InterfaceRecord) -> bool {
    current.kind != next.kind
        || current.enabled != next.enabled
        || current.host != next.host
        || current.port != next.port
        || (current.kind == "udp" && udp_forward_addr(current) != udp_forward_addr(next))
}

fn tcp_endpoint(record: &InterfaceRecord) -> Option<String> {
    Some(format!("{}:{}", record.host.as_ref()?, record.port?))
}

fn udp_bind_and_forward_addr(
    record: &InterfaceRecord,
) -> Result<(String, Option<String>), io::Error> {
    let host = record.host.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(
        || io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires host"),
    )?;
    if setting_str(record, "device").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support device-bound records",
        ));
    }
    let port = record.port.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires port")
    })?;
    Ok((format!("{host}:{port}"), udp_forward_addr_result(record)?))
}

fn udp_forward_addr(record: &InterfaceRecord) -> Option<String> {
    udp_forward_addr_result(record).ok().flatten()
}

fn udp_forward_addr_result(record: &InterfaceRecord) -> Result<Option<String>, io::Error> {
    let host = setting_str(record, "target_host").or_else(|| setting_str(record, "forward_ip"));
    let port = setting_u64(record, "target_port").or_else(|| setting_u64(record, "forward_port"));
    let (host, port) = match (host, port) {
        (Some(host), Some(port)) => (host, port),
        (None, None) => return Ok(None),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "udp hot-apply target_host and target_port must be provided together",
            ))
        }
    };
    if host_is_multicast(host) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support multicast targets",
        ));
    }
    let port = u16::try_from(port).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply target_port is out of range")
    })?;
    Ok(Some(format!("{host}:{port}")))
}

fn mark_udp_record_runtime_status(
    record: &mut InterfaceRecord,
    runtime_iface: Option<AddressHash>,
) {
    if let Ok((bind_addr, forward_addr)) = udp_bind_and_forward_addr(record) {
        let role = if record.host.as_deref().is_some_and(host_is_multicast) {
            "multicast"
        } else if forward_addr.is_some() {
            "peer"
        } else {
            "listener"
        };
        let iface = runtime_iface.map(|value| value.to_string());
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            runtime.insert(
                "udp".to_string(),
                serde_json::json!({
                    "status": {
                        "link_state": "configured",
                        "role": role,
                        "bind_addr": bind_addr,
                        "forward_addr": forward_addr,
                        "iface": iface,
                    }
                }),
            );
        });
    }
}

fn host_is_multicast(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_multicast())
}

fn apply_record_runtime_config(
    manager: &mut InterfaceManager,
    address: AddressHash,
    record: &InterfaceRecord,
) {
    manager.set_mode(address, interface_record_mode(record));
    manager.set_outgoing(address, setting_bool(record, "outgoing").unwrap_or(true));
    manager.set_announce_pacing(
        address,
        setting_u64(record, "bitrate").unwrap_or(62_500),
        setting_u64(record, "announce_cap").unwrap_or(2),
    );
    manager.set_shared_config(address, interface_record_shared_config(record));
}

fn interface_record_mode(record: &InterfaceRecord) -> InterfaceMode {
    setting_str(record, "interface_mode")
        .or_else(|| setting_str(record, "mode"))
        .and_then(|value| InterfaceMode::parse(value).ok().flatten())
        .unwrap_or(InterfaceMode::Full)
}

#[cfg(test)]
#[path = "tests/interface_hot_apply.rs"]
mod tests;
