use rns_rpc::{InterfaceMutationBridge, InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::pipe::{PipeInterface, PipeRuntimeStatusHandle};
use rns_transport::iface::tcp_client::{TcpClient, TcpRuntimeStatusHandle};
use rns_transport::iface::tcp_server::{TcpListenerRuntimeStatusHandle, TcpServer};
use rns_transport::iface::udp::{UdpInterface, UdpRuntimeStatusHandle};
use rns_transport::iface::{IfaceRole, InterfaceManager};
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use tokio::sync::mpsc::{channel, error::TrySendError, Receiver, Sender};

use crate::bootstrap::{
    mark_interface_runtime_fields, mark_interface_runtime_managed, mark_interface_startup_status,
};
use interface_hot_apply_parts::ifac_validation::validate_hot_apply_ifac_configuration;
#[cfg(test)]
use interface_hot_apply_parts::pipe_runtime_refresh::refresh_hot_apply_pipe_runtime_status_once;
use interface_hot_apply_parts::pipe_runtime_refresh::{
    attach_hot_apply_pipe_runtime_status, spawn_hot_apply_pipe_runtime_status_refresher,
    HotApplyPipeRefresh,
};
pub(crate) use interface_hot_apply_parts::record_hot_apply::mark_tcp_server_record_runtime_status;
use interface_hot_apply_parts::record_hot_apply::{
    apply_record_runtime_config, hot_apply_interface_key, hot_apply_interface_record_changed,
    hot_apply_interface_seed_key as record_hot_apply_interface_seed_key, interface_record_mode,
    mark_pipe_record_runtime_status, mark_udp_record_runtime_status, pipe_adapter, tcp_endpoint,
    tcp_server_bind_addr, tcp_server_client_mtu, udp_bind_and_forward_addr,
    validate_hot_apply_uniqueness,
};
#[cfg(test)]
use interface_hot_apply_parts::record_hot_apply::{
    tcp_server_bind_addr_with_device_resolver, udp_bind_and_forward_addr_with_device_resolver,
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
#[path = "interface_hot_apply_parts/named_management.rs"]
mod named_management;
#[path = "interface_hot_apply_parts/spawn_interface.rs"]
mod spawn_interface;
use named_management::stop_hot_apply_interface;
use spawn_interface::spawn_hot_apply_interface;
#[path = "interface_hot_apply_parts/apply_records.rs"]
mod apply_records;
use apply_records::apply_hot_apply_interface_records;
#[path = "interface_hot_apply_parts/discovery_runtime.rs"]
mod discovery_runtime;
#[path = "interface_hot_apply_parts/record_publication.rs"]
mod record_publication;
pub(super) use discovery_runtime::{DiscoveryObservationSender, NativeDiscoveryConfig};
use discovery_runtime::{ManagedInterfaceOrigin, NativeDiscovery};
#[path = "interface_hot_apply_parts/managed_interface_key.rs"]
mod managed_interface_key;
use managed_interface_key::ManagedInterfaceKey;

fn configured_key(name: &str) -> ManagedInterfaceKey {
    ManagedInterfaceKey::Configured(name.to_owned())
}

#[derive(Clone)]
pub(super) struct InterfaceHotApplyBridge {
    tx: Sender<InterfaceHotApplyCommand>,
    config_path: Option<PathBuf>,
    management_enabled: bool,
    record_publisher: Option<Weak<RpcDaemon>>,
    #[cfg(test)]
    tcp_listener_refreshes: Arc<StdMutex<HashMap<String, HotApplyTcpListenerRefresh>>>,
    #[cfg(test)]
    udp_refreshes: Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
    #[cfg(test)]
    pipe_refreshes: Arc<StdMutex<HashMap<String, HotApplyPipeRefresh>>>,
}

const INTERFACE_HOT_APPLY_QUEUE_CAPACITY: usize = 64;

#[derive(Clone)]
struct HotApplyRuntimeRefreshes {
    tcp_listeners: Arc<StdMutex<HashMap<String, HotApplyTcpListenerRefresh>>>,
    udp: Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
    pipe: Arc<StdMutex<HashMap<String, HotApplyPipeRefresh>>>,
}

impl HotApplyRuntimeRefreshes {
    fn new() -> Self {
        Self {
            tcp_listeners: Arc::new(StdMutex::new(HashMap::new())),
            udp: Arc::new(StdMutex::new(HashMap::new())),
            pipe: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    fn spawn_refreshers(&self, daemon: Weak<RpcDaemon>) {
        spawn_hot_apply_tcp_listener_runtime_status_refresher(
            daemon.clone(),
            self.tcp_listeners.clone(),
        );
        spawn_hot_apply_udp_runtime_status_refresher(daemon.clone(), self.udp.clone());
        spawn_hot_apply_pipe_runtime_status_refresher(daemon, self.pipe.clone());
    }
}

impl InterfaceHotApplyBridge {
    pub(super) fn with_management_config(
        mut self,
        config_path: Option<PathBuf>,
        enabled: bool,
    ) -> Self {
        self.config_path = config_path;
        self.management_enabled = enabled;
        self
    }

    #[cfg(test)]
    pub(super) fn spawn(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    ) -> Self {
        Self::spawn_inner(iface_manager, seeded, None, None, None)
    }

    #[cfg(test)]
    pub(super) fn spawn_with_daemon(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
    ) -> Self {
        Self::spawn_inner(iface_manager, seeded, None, Some(daemon), None)
    }

    #[cfg(test)]
    pub(super) fn spawn_with_transport_and_daemon(
        transport: Arc<Transport>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
    ) -> Self {
        Self::spawn_with_discovery(transport, seeded, daemon, None)
    }

    pub(super) fn spawn_with_discovery(
        transport: Arc<Transport>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
        discovery: Option<NativeDiscoveryConfig>,
    ) -> Self {
        Self::spawn_inner(
            transport.iface_manager(),
            seeded,
            Some(transport),
            Some(daemon),
            discovery,
        )
    }

    pub(super) fn discovery_observations(&self) -> DiscoveryObservationSender {
        DiscoveryObservationSender::new(self.tx.downgrade())
    }

    fn spawn_inner(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        transport: Option<Arc<Transport>>,
        daemon: Option<Weak<RpcDaemon>>,
        discovery: Option<NativeDiscoveryConfig>,
    ) -> Self {
        let (tx, rx) = channel(INTERFACE_HOT_APPLY_QUEUE_CAPACITY);
        let record_publisher = daemon.clone();
        let refreshes = HotApplyRuntimeRefreshes::new();
        if let Some(daemon) = daemon.clone() {
            refreshes.spawn_refreshers(daemon);
        }
        tokio::spawn(run_interface_mutation_worker(
            iface_manager,
            rx,
            seeded,
            transport.clone(),
            refreshes.clone(),
            daemon,
            discovery,
        ));
        Self {
            tx,
            config_path: None,
            management_enabled: true,
            record_publisher,
            #[cfg(test)]
            tcp_listener_refreshes: refreshes.tcp_listeners,
            #[cfg(test)]
            udp_refreshes: refreshes.udp,
            #[cfg(test)]
            pipe_refreshes: refreshes.pipe,
        }
    }
}

impl InterfaceHotApplyBridge {
    fn queue_interfaces(
        &self,
        mut interfaces: Vec<InterfaceRecord>,
        reload_timestamp: Option<i64>,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        // Runtime metadata is native output, never configuration authority.
        for record in &mut interfaces {
            if let Some(settings) =
                record.settings.as_mut().and_then(serde_json::Value::as_object_mut)
            {
                settings.remove("_runtime");
            }
        }
        validate_hot_apply_uniqueness(&interfaces)?;
        validate_hot_apply_ifac_configuration(&interfaces)?;
        let effective = interfaces
            .iter()
            .cloned()
            .map(|mut record| {
                if matches!(record.kind.as_str(), "tcp_client" | "tcp_server" | "udp" | "pipe")
                    && record.enabled
                {
                    mark_interface_startup_status(&mut record, "spawned", None, None);
                    mark_interface_runtime_managed(&mut record, "daemon_transport");
                    mark_interface_runtime_fields(&mut record, "running", 0);
                    match record.kind.as_str() {
                        "tcp_server" => mark_tcp_server_record_runtime_status(&mut record, None),
                        "udp" => mark_udp_record_runtime_status(&mut record, None),
                        "pipe" => mark_pipe_record_runtime_status(&mut record, None),
                        _ => {}
                    }
                }
                record
            })
            .collect::<Vec<_>>();
        self.tx
            .try_send(InterfaceHotApplyCommand::Apply { interfaces, reload_timestamp })
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "interface mutation queue is full")
                }
                TrySendError::Closed(_) => io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "interface mutation worker is not running",
                ),
            })?;
        Ok(effective)
    }
}

impl InterfaceMutationBridge for InterfaceHotApplyBridge {
    fn publishes_interface_records(&self) -> bool {
        self.record_publisher.as_ref().and_then(Weak::upgrade).is_some()
    }

    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        self.queue_interfaces(interfaces, None)
    }

    fn reload_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
        timestamp: i64,
    ) -> Result<Vec<InterfaceRecord>, io::Error> {
        self.queue_interfaces(interfaces, Some(timestamp))
    }

    fn manage_named_interface(
        &self,
        operation: &str,
        name: &str,
    ) -> Result<serde_json::Value, io::Error> {
        named_management::dispatch(self, operation, name)
    }
}

enum InterfaceHotApplyCommand {
    Apply {
        interfaces: Vec<InterfaceRecord>,
        reload_timestamp: Option<i64>,
    },
    Manage {
        operation: String,
        name: String,
        configured: Option<InterfaceRecord>,
        reply: std::sync::mpsc::SyncSender<Result<serde_json::Value, io::Error>>,
    },
    DiscoveryObserved(Box<rns_transport::discovery::DiscoveredInterface>),
}

#[derive(Clone)]
struct ManagedHotApplyInterface {
    record: InterfaceRecord,
    address: AddressHash,
    runtime_status: Option<HotApplyRuntimeStatus>,
    origin: ManagedInterfaceOrigin,
}

async fn run_interface_mutation_worker(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    mut rx: Receiver<InterfaceHotApplyCommand>,
    seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    transport: Option<Arc<Transport>>,
    refreshes: HotApplyRuntimeRefreshes,
    daemon: Option<Weak<RpcDaemon>>,
    discovery: Option<NativeDiscoveryConfig>,
) {
    let mut managed = seeded
        .into_iter()
        .map(|(key, record, address)| {
            let origin = ManagedInterfaceOrigin::configured(&record);
            (key.into(), ManagedHotApplyInterface { record, address, runtime_status: None, origin })
        })
        .collect::<HashMap<ManagedInterfaceKey, _>>();

    let mut discovery = discovery.map(NativeDiscovery::new);
    if let (Some(discovery), Some(transport), Some(daemon)) =
        (discovery.as_mut(), transport.as_ref(), daemon.as_ref().and_then(Weak::upgrade))
    {
        discovery.initial(&iface_manager, &mut managed, transport, &daemon).await;
    }
    let mut monitor = tokio::time::interval_at(
        tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(5),
    );
    monitor.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let command = tokio::select! {
            command = rx.recv() => match command { Some(command) => command, None => break },
            _ = monitor.tick(), if discovery.is_some() => {
                if let (Some(discovery), Some(transport), Some(daemon)) = (
                    discovery.as_mut(), transport.as_ref(), daemon.as_ref().and_then(Weak::upgrade),
                ) {
                    discovery.monitor(&iface_manager, &mut managed, transport, &refreshes, &daemon).await;
                }
                continue;
            }
        };
        match command {
            InterfaceHotApplyCommand::Apply { interfaces, reload_timestamp } => {
                let interfaces = record_publication::preserve_unmanaged_records(
                    interfaces,
                    daemon.as_ref().and_then(Weak::upgrade).as_deref(),
                );
                let applied = apply_hot_apply_interface_records(
                    &iface_manager,
                    &mut managed,
                    interfaces,
                    transport.as_ref(),
                    &refreshes,
                    daemon.as_ref(),
                )
                .await;
                if let Some(daemon) = daemon.as_ref().and_then(Weak::upgrade) {
                    daemon.replace_interfaces(applied.clone());
                    daemon.publish_event(rns_rpc::RpcEvent {
                        event_type: "interfaces_updated".to_string(),
                        payload: serde_json::json!({
                            "interfaces": applied, "queued": false, "complete": true,
                        }),
                    });
                    if let Some(timestamp) = reload_timestamp {
                        daemon.publish_event(rns_rpc::RpcEvent {
                            event_type: "config_reloaded".to_string(),
                            payload: serde_json::json!({ "timestamp": timestamp }),
                        });
                    }
                }
            }
            InterfaceHotApplyCommand::Manage { operation, name, configured, reply } => {
                let endpoint =
                    configured.as_ref().and_then(discovery_runtime::record_endpoint_hash);
                let discovered_key = discovery_runtime::key_for_discovered_name(&managed, &name);
                let discovered = discovered_key
                    .as_ref()
                    .and_then(|key| managed.get(key))
                    .and_then(|active| active.origin.endpoint_hash());
                if operation == "detach" {
                    if let (Some(discovery), Some(endpoint)) = (discovery.as_mut(), discovered) {
                        if let Some(key) = &discovered_key {
                            discovery.suppress(endpoint, key);
                        }
                    }
                }
                let result = if operation == "detach" && discovered.is_some() {
                    discovery_runtime::detach_discovered(
                        &iface_manager,
                        &mut managed,
                        &name,
                        transport.as_ref(),
                        daemon.as_ref(),
                    )
                    .await
                } else {
                    named_management::apply(
                        &iface_manager,
                        &mut managed,
                        named_management::NamedInterfaceAction::new(&operation, &name, configured),
                        transport.as_ref(),
                        &refreshes,
                        daemon.as_ref(),
                    )
                    .await
                };
                if let Some(discovery) = discovery.as_mut() {
                    if result.is_ok() {
                        if let Some(endpoint) = endpoint.filter(|_| operation != "detach") {
                            discovery.allow(endpoint);
                        }
                    }
                }
                if reply.send(result).is_err() {
                    log::warn!(
                        "[daemon] named interface management reply was not received operation={operation} name={name}"
                    );
                }
            }
            InterfaceHotApplyCommand::DiscoveryObserved(info) => {
                if let (Some(discovery), Some(transport), Some(daemon)) = (
                    discovery.as_mut(),
                    transport.as_ref(),
                    daemon.as_ref().and_then(Weak::upgrade),
                ) {
                    discovery.admit(&info, &iface_manager, &mut managed, transport, &daemon).await;
                }
            }
        }
    }
    for (_, active) in managed.drain() {
        if let Err(error) =
            discovery_runtime::stop_and_join(&iface_manager, transport.as_ref(), active.address)
                .await
        {
            log::warn!(
                "[daemon] mutation owner shutdown did not confirm worker exit address={}: {error}",
                active.address
            );
        }
    }
}

#[derive(Clone)]
enum HotApplyRuntimeStatus {
    TcpClient(TcpRuntimeStatusHandle),
    TcpListener(TcpListenerRuntimeStatusHandle),
    Udp(UdpRuntimeStatusHandle),
    Pipe(PipeRuntimeStatusHandle),
}

pub(super) fn hot_apply_interface_seed_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind == "backbone_client" {
        record
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| Some(format!("backbone:{}:{}", record.host.as_deref()?, record.port?)))
    } else {
        record_hot_apply_interface_seed_key(record)
    }
}

#[cfg(test)]
#[path = "tests/interface_hot_apply.rs"]
mod tests;
