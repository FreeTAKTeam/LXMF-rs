use rns_rpc::{InterfaceMutationBridge, InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::{TcpListenerRuntimeStatusHandle, TcpServer};
use rns_transport::iface::udp::{UdpInterface, UdpRuntimeStatusHandle};
use rns_transport::iface::{IfaceRole, InterfaceManager};
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use tokio::sync::mpsc::{channel, error::TrySendError, Receiver, Sender};

use crate::bootstrap::{
    mark_interface_runtime_fields, mark_interface_runtime_managed, mark_interface_startup_status,
};
use interface_hot_apply_parts::record_hot_apply::{
    apply_record_runtime_config, hot_apply_interface_key, hot_apply_interface_record_changed,
    hot_apply_interface_seed_key as record_hot_apply_interface_seed_key, interface_record_mode,
    mark_tcp_server_record_runtime_status, mark_udp_record_runtime_status, tcp_endpoint,
    tcp_server_bind_addr, tcp_server_client_mtu, udp_bind_and_forward_addr,
    validate_hot_apply_uniqueness,
};
#[cfg(test)]
use interface_hot_apply_parts::tcp_runtime_refresh::refresh_hot_apply_tcp_listener_runtime_status_once;
use interface_hot_apply_parts::tcp_runtime_refresh::{
    attach_hot_apply_tcp_listener_runtime_status,
    spawn_hot_apply_tcp_listener_runtime_status_refresher, HotApplyTcpListenerRefresh,
};
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
    tcp_listener_refreshes: Arc<StdMutex<HashMap<String, HotApplyTcpListenerRefresh>>>,
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
        let tcp_listener_refreshes = Arc::new(StdMutex::new(HashMap::new()));
        let udp_refreshes = Arc::new(StdMutex::new(HashMap::new()));
        if let Some(daemon) = daemon.clone() {
            spawn_hot_apply_tcp_listener_runtime_status_refresher(
                daemon.clone(),
                tcp_listener_refreshes.clone(),
            );
            spawn_hot_apply_udp_runtime_status_refresher(daemon, udp_refreshes.clone());
        }
        tokio::spawn(run_interface_mutation_worker(
            iface_manager,
            rx,
            seeded,
            transport.clone(),
            tcp_listener_refreshes.clone(),
            udp_refreshes.clone(),
            daemon,
        ));
        Self {
            tx,
            #[cfg(test)]
            tcp_listener_refreshes,
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
                if matches!(record.kind.as_str(), "tcp_client" | "tcp_server" | "udp")
                    && record.enabled
                {
                    mark_interface_startup_status(&mut record, "spawned", None, None);
                    mark_interface_runtime_managed(&mut record, "daemon_transport");
                    mark_interface_runtime_fields(&mut record, "running", 0);
                    match record.kind.as_str() {
                        "tcp_server" => mark_tcp_server_record_runtime_status(&mut record, None),
                        "udp" => mark_udp_record_runtime_status(&mut record, None),
                        _ => {}
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
    tcp_listener_refreshes: Arc<StdMutex<HashMap<String, HotApplyTcpListenerRefresh>>>,
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
                    &tcp_listener_refreshes,
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
    tcp_listener_refreshes: &Arc<StdMutex<HashMap<String, HotApplyTcpListenerRefresh>>>,
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
                tcp_listener_refreshes
                    .lock()
                    .expect("tcp listener refresh mutex poisoned")
                    .remove(&key);
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
        if let Some((address, runtime_status)) =
            spawn_hot_apply_interface(iface_manager, transport, &record).await
        {
            match runtime_status {
                Some(HotApplyRuntimeStatus::TcpListener(status)) => {
                    attach_hot_apply_tcp_listener_runtime_status(daemon, &record, address, &status);
                    tcp_listener_refreshes
                        .lock()
                        .expect("tcp listener refresh mutex poisoned")
                        .insert(
                            key.clone(),
                            HotApplyTcpListenerRefresh {
                                record: record.clone(),
                                runtime_iface: address,
                                status,
                            },
                        );
                }
                Some(HotApplyRuntimeStatus::Udp(status)) => {
                    status.update(|status| {
                        status.iface = Some(address.to_string());
                    });
                    attach_hot_apply_udp_runtime_status(daemon, &record, address, &status);
                    udp_refreshes.lock().expect("udp refresh mutex poisoned").insert(
                        key.clone(),
                        HotApplyUdpRefresh {
                            record: record.clone(),
                            runtime_iface: address,
                            status,
                        },
                    );
                }
                None => {}
            }
            managed.insert(key, ManagedHotApplyInterface { record, address });
        }
    }
}

enum HotApplyRuntimeStatus {
    TcpListener(TcpListenerRuntimeStatusHandle),
    Udp(UdpRuntimeStatusHandle),
}

pub(super) fn hot_apply_interface_seed_key(record: &InterfaceRecord) -> Option<String> {
    record_hot_apply_interface_seed_key(record)
}

async fn spawn_hot_apply_interface(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    record: &InterfaceRecord,
) -> Option<(AddressHash, Option<HotApplyRuntimeStatus>)> {
    let mode = interface_record_mode(record);
    let (address, runtime_status) = match record.kind.as_str() {
        "tcp_client" => {
            let address = {
                let mut manager = iface_manager.lock().await;
                manager.spawn_as_with_mode(
                    TcpClient::new(tcp_endpoint(record)?),
                    TcpClient::spawn,
                    IfaceRole::Unicast,
                    mode,
                )
            };
            (address, None)
        }
        "tcp_server" => {
            let bind_addr = match tcp_server_bind_addr(record) {
                Ok(bind_addr) => bind_addr,
                Err(error) => {
                    log::warn!(
                        "[daemon] hot-apply tcp_server rejected invalid bind address: {}",
                        error
                    );
                    return None;
                }
            };
            let mut adapter = TcpServer::new(bind_addr, iface_manager.clone());
            if let Some(client_mtu) = tcp_server_client_mtu(record) {
                adapter = adapter.with_client_mtu(client_mtu);
            }
            let status = adapter.runtime_status_handle();
            let address = {
                let mut manager = iface_manager.lock().await;
                manager.spawn_as_with_mode(adapter, TcpServer::spawn, IfaceRole::Unicast, mode)
            };
            (address, Some(HotApplyRuntimeStatus::TcpListener(status)))
        }
        "udp" => {
            let (bind_addr, forward_addr) = match udp_bind_and_forward_addr(record) {
                Ok(addresses) => addresses,
                Err(error) => {
                    log::warn!(
                        "[daemon] hot-apply udp rejected invalid bind/forward address: {error}"
                    );
                    return None;
                }
            };
            let adapter = UdpInterface::new(bind_addr.clone(), forward_addr.clone());
            if adapter.is_multicast() {
                let transport = transport?;
                let (address, status) = transport
                    .add_multicast_udp_interface_with_status(bind_addr, forward_addr)
                    .await;
                (address, Some(HotApplyRuntimeStatus::Udp(status)))
            } else {
                let status = adapter.runtime_status_handle();
                (
                    iface_manager.lock().await.spawn_as_with_mode(
                        adapter,
                        UdpInterface::spawn,
                        IfaceRole::Unicast,
                        mode,
                    ),
                    Some(HotApplyRuntimeStatus::Udp(status)),
                )
            }
        }
        _ => return None,
    };
    {
        let mut guard = iface_manager.lock().await;
        apply_record_runtime_config(&mut guard, address, record);
    }
    Some((address, runtime_status))
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

#[cfg(test)]
#[path = "tests/interface_hot_apply.rs"]
mod tests;
