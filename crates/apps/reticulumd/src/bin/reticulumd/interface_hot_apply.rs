use crate::configured_interface_runtime::NativeRuntimeStatus as HotApplyRuntimeStatus;
use crate::configured_interface_runtime::{ConfiguredInterfaceStartup, NativeInterfaceRuntime};
use rns_rpc::{InterfaceMutationBridge, InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::pipe::PipeInterface;
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::tcp_server::TcpServer;
use rns_transport::iface::udp::UdpInterface;
use rns_transport::iface::{IfaceRole, InterfaceManager};
use rns_transport::transport::Transport;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex as StdMutex;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc::{channel, error::TrySendError, Receiver, Sender};

use crate::bootstrap::{
    mark_interface_runtime_fields, mark_interface_runtime_managed, mark_interface_startup_status,
    ConfiguredStartupRuntime,
};
use interface_hot_apply_parts::ifac_validation::validate_hot_apply_ifac_configuration;
#[cfg(test)]
use interface_hot_apply_parts::pipe_runtime_refresh::refresh_hot_apply_pipe_runtime_status_once;
#[cfg(test)]
use interface_hot_apply_parts::pipe_runtime_refresh::HotApplyPipeRefresh;
pub(crate) use interface_hot_apply_parts::record_hot_apply::mark_tcp_server_record_runtime_status;
use interface_hot_apply_parts::record_hot_apply::{
    apply_record_runtime_config, hot_apply_interface_key, hot_apply_interface_record_changed,
    hot_apply_interface_seed_key as record_hot_apply_interface_seed_key, interface_record_mode,
    mark_pipe_record_runtime_status, mark_udp_record_runtime_status, pipe_adapter,
    tcp_server_bind_addr, tcp_server_client_mtu, udp_bind_and_forward_addr,
    validate_hot_apply_uniqueness,
};
#[cfg(test)]
use interface_hot_apply_parts::record_hot_apply::{
    tcp_server_bind_addr_with_device_resolver, udp_bind_and_forward_addr_with_device_resolver,
};
#[cfg(test)]
use interface_hot_apply_parts::tcp_runtime_refresh::refresh_hot_apply_tcp_listener_runtime_status_once;
#[cfg(test)]
use interface_hot_apply_parts::tcp_runtime_refresh::HotApplyTcpListenerRefresh;
#[cfg(test)]
use interface_hot_apply_parts::udp_runtime_refresh::refresh_hot_apply_udp_runtime_status_once;
#[cfg(test)]
use interface_hot_apply_parts::udp_runtime_refresh::HotApplyUdpRefresh;

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
#[path = "interface_hot_apply_parts/native_shutdown.rs"]
mod native_shutdown;
#[cfg(test)]
pub(crate) use native_shutdown::stop_owned_native;
#[path = "interface_hot_apply_parts/record_publication.rs"]
mod record_publication;
pub(super) use discovery_runtime::{DiscoveryObservationSender, NativeDiscoveryConfig};
use discovery_runtime::{ManagedInterfaceOrigin, NativeDiscovery};
#[path = "interface_hot_apply_parts/managed_interface_key.rs"]
mod managed_interface_key;
use managed_interface_key::ManagedInterfaceKey;

#[derive(Clone, Copy)]
struct NativeInterfaceContext<'a> {
    iface_manager: &'a Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&'a Arc<Transport>>,
    reconnect_events: Option<&'a Sender<AddressHash>>,
}

#[cfg(test)]
#[path = "tests/interface_hot_apply/tcp_options.rs"]
mod tcp_options_tests;

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

pub(super) struct InterfaceMutationOwner {
    pub(super) bridge: InterfaceHotApplyBridge,
    completion: Option<tokio::task::JoinHandle<io::Result<()>>>,
}

impl InterfaceMutationOwner {
    pub(super) fn with_management_config(
        mut self,
        config_path: Option<PathBuf>,
        enabled: bool,
    ) -> Self {
        self.bridge = self.bridge.with_management_config(config_path, enabled);
        self
    }

    pub(super) fn discovery_observations(&self) -> DiscoveryObservationSender {
        self.bridge.discovery_observations()
    }

    pub(super) async fn shutdown(&mut self) -> io::Result<()> {
        let Some(worker) = self.completion.as_mut() else {
            return Ok(());
        };
        // Keep the original handle in the supervisor if this await is cancelled.
        if self.bridge.tx.send(InterfaceHotApplyCommand::Shutdown).await.is_err() {
            log::debug!("[daemon] interface mutation owner already closed during shutdown");
        }
        let result = worker.await.map_err(io::Error::other).and_then(|result| result);
        self.completion = None;
        result
    }
}

const INTERFACE_HOT_APPLY_QUEUE_CAPACITY: usize = 64;

#[derive(Clone)]
struct HotApplyRuntimeRefreshes {
    #[cfg(test)]
    tcp_listeners: Arc<StdMutex<HashMap<String, HotApplyTcpListenerRefresh>>>,
    #[cfg(test)]
    udp: Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
    #[cfg(test)]
    pipe: Arc<StdMutex<HashMap<String, HotApplyPipeRefresh>>>,
}

impl HotApplyRuntimeRefreshes {
    fn new() -> Self {
        Self {
            #[cfg(test)]
            tcp_listeners: Arc::new(StdMutex::new(HashMap::new())),
            #[cfg(test)]
            udp: Arc::new(StdMutex::new(HashMap::new())),
            #[cfg(test)]
            pipe: Arc::new(StdMutex::new(HashMap::new())),
        }
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
        Self::spawn_inner(iface_manager, seeded, None, None, None, None, None).0
    }

    #[cfg(test)]
    pub(super) fn spawn_with_daemon(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
    ) -> Self {
        Self::spawn_inner(iface_manager, seeded, None, Some(daemon), None, None, None).0
    }

    #[cfg(test)]
    pub(super) fn spawn_with_transport_and_daemon(
        transport: Arc<Transport>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        daemon: Weak<RpcDaemon>,
    ) -> Self {
        Self::spawn_with_discovery(transport, seeded, daemon, None)
    }

    #[cfg(test)]
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
            None,
            None,
        )
        .0
    }

    pub(super) fn discovery_observations(&self) -> DiscoveryObservationSender {
        DiscoveryObservationSender::new(self.tx.downgrade())
    }

    pub(super) fn spawn_with_configured_roots(
        transport: Arc<Transport>,
        startup: Vec<ConfiguredInterfaceStartup>,
        daemon: Weak<RpcDaemon>,
        discovery: Option<NativeDiscoveryConfig>,
        configured_startup: Option<ConfiguredStartupRuntime>,
    ) -> InterfaceMutationOwner {
        let (bridge, completion) = Self::spawn_inner(
            transport.iface_manager(),
            Vec::new(),
            Some(transport),
            Some(daemon),
            discovery,
            Some(startup),
            configured_startup,
        );
        InterfaceMutationOwner { bridge, completion: Some(completion) }
    }

    fn spawn_inner(
        iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
        seeded: Vec<(String, InterfaceRecord, AddressHash)>,
        transport: Option<Arc<Transport>>,
        daemon: Option<Weak<RpcDaemon>>,
        discovery: Option<NativeDiscoveryConfig>,
        startup: Option<Vec<ConfiguredInterfaceStartup>>,
        configured_startup: Option<ConfiguredStartupRuntime>,
    ) -> (Self, tokio::task::JoinHandle<io::Result<()>>) {
        let (tx, rx) = channel(INTERFACE_HOT_APPLY_QUEUE_CAPACITY);
        let record_publisher = daemon.clone();
        let refreshes = HotApplyRuntimeRefreshes::new();
        let managed = mutation_worker::initial_interfaces(seeded, startup);
        if let Some(daemon) = daemon.as_ref().and_then(Weak::upgrade) {
            record_publication::publish_control_bindings(&managed, &daemon);
        }
        let completion = tokio::spawn(run_interface_mutation_worker(
            iface_manager,
            rx,
            managed,
            mutation_worker::MutationRuntime {
                transport,
                refreshes: refreshes.clone(),
                daemon,
                discovery,
                configured_startup,
            },
        ));
        let bridge = Self {
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
        };
        (bridge, completion)
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
    Shutdown,
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

struct ConfiguredRecordBinding {
    recipe_index: usize,
    position: usize,
}

struct ManagedHotApplyInterface {
    record: InterfaceRecord,
    native: Option<NativeInterfaceRuntime>,
    configured: Option<ConfiguredRecordBinding>,
    origin: ManagedInterfaceOrigin,
}

impl ManagedHotApplyInterface {
    fn active(
        record: InterfaceRecord,
        address: AddressHash,
        status: Option<HotApplyRuntimeStatus>,
        origin: ManagedInterfaceOrigin,
    ) -> Self {
        Self {
            record,
            native: Some(NativeInterfaceRuntime::new(address, status)),
            configured: None,
            origin,
        }
    }
}

#[path = "interface_hot_apply_parts/mutation_worker.rs"]
mod mutation_worker;
use mutation_worker::run_interface_mutation_worker;

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

#[cfg(test)]
#[path = "tests/interface_hot_apply/configured_shutdown.rs"]
mod configured_shutdown_tests;

#[cfg(test)]
#[path = "tests/interface_hot_apply/configured_projection.rs"]
mod configured_projection_tests;

#[cfg(test)]
#[path = "tests/interface_hot_apply/configured_controls.rs"]
mod configured_controls_tests;
