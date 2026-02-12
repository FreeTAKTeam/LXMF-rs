use crate::cli::daemon::DaemonStatus;
use crate::cli::profile::{
    load_profile_settings, load_reticulum_config, profile_paths, resolve_identity_path,
    resolve_runtime_profile_name, InterfaceEntry, ProfilePaths, ProfileSettings,
};
use crate::helpers::{display_name_from_app_data, is_msgpack_array_prefix, normalize_display_name};
use crate::message::Message;
use crate::LxmfError;
use rand_core::OsRng;
use reticulum::destination::link::{LinkEvent, LinkStatus};
use reticulum::destination::{DestinationDesc, DestinationName, SingleInputDestination};
use reticulum::hash::AddressHash;
use reticulum::identity::{Identity, PrivateIdentity};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use reticulum::rpc::{
    AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon, RpcEvent, RpcRequest,
};
use reticulum::storage::messages::{MessageRecord, MessagesStore};
use reticulum::transport::{
    DeliveryReceipt, ReceiptHandler, SendPacketOutcome, Transport, TransportConfig,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;
use tokio::task::LocalSet;

const INFERRED_TRANSPORT_BIND: &str = "127.0.0.1:0";
const DEFAULT_ANNOUNCE_INTERVAL_SECS: u64 = 900;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub profile: String,
    pub rpc: Option<String>,
    pub transport: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self { profile: "default".to_string(), rpc: None, transport: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbeReport {
    pub profile: String,
    pub local: DaemonStatus,
    pub rpc: RpcProbeReport,
    pub events: EventsProbeReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcProbeReport {
    pub reachable: bool,
    pub endpoint: String,
    pub method: Option<String>,
    pub roundtrip_ms: Option<u128>,
    pub identity_hash: Option<String>,
    pub status: Option<serde_json::Value>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventsProbeReport {
    pub reachable: bool,
    pub endpoint: String,
    pub roundtrip_ms: Option<u128>,
    pub event_type: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct RuntimeHandle {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    profile: String,
    settings: ProfileSettings,
    running: AtomicBool,
    next_id: AtomicU64,
    transport: Option<String>,
    transport_inferred: bool,
    log_path: String,
    command_tx: UnboundedSender<RuntimeRequest>,
}

struct RuntimeRequest {
    command: RuntimeCommand,
    respond_to: std_mpsc::Sender<Result<RuntimeResponse, String>>,
}

enum RuntimeCommand {
    Status,
    Call(RpcRequest),
    PollEvent,
    Stop,
}

enum RuntimeResponse {
    Status(DaemonStatus),
    Value(Value),
    Event(Option<RpcEvent>),
    Ack,
}

struct WorkerInit {
    profile: String,
    settings: ProfileSettings,
    paths: ProfilePaths,
    transport: Option<String>,
    transport_inferred: bool,
    interfaces: Vec<InterfaceEntry>,
}

struct WorkerState {
    profile: String,
    status_template: DaemonStatus,
    daemon: Rc<RpcDaemon>,
    shutdown_tx: watch::Sender<bool>,
    scheduler_handle: Option<tokio::task::JoinHandle<()>>,
    shutdown: bool,
}

#[derive(Clone, Copy)]
struct PeerCrypto {
    identity: Identity,
}

struct EmbeddedTransportBridge {
    transport: Arc<Transport>,
    signer: PrivateIdentity,
    delivery_source_hash: [u8; 16],
    announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    announce_app_data: Option<Vec<u8>>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

#[derive(Debug, Clone)]
struct ReceiptEvent {
    message_id: String,
    status: String,
}

#[derive(Clone)]
struct ReceiptBridge {
    map: Arc<Mutex<HashMap<String, String>>>,
    tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

impl RuntimeHandle {
    pub fn status(&self) -> DaemonStatus {
        match self.request(RuntimeCommand::Status) {
            Ok(RuntimeResponse::Status(status)) => {
                self.inner.running.store(status.running, Ordering::Relaxed);
                status
            }
            _ => {
                self.inner.running.store(false, Ordering::Relaxed);
                self.fallback_status()
            }
        }
    }

    pub fn profile(&self) -> &str {
        &self.inner.profile
    }

    pub fn settings(&self) -> ProfileSettings {
        self.inner.settings.clone()
    }

    pub fn stop(&self) {
        if !self.inner.running.swap(false, Ordering::Relaxed) {
            return;
        }
        let _ = self.request(RuntimeCommand::Stop);
    }

    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::Relaxed)
    }

    pub fn poll_event(&self) -> Option<RpcEvent> {
        if !self.is_running() {
            return None;
        }

        match self.request(RuntimeCommand::PollEvent) {
            Ok(RuntimeResponse::Event(event)) => event,
            _ => {
                self.inner.running.store(false, Ordering::Relaxed);
                None
            }
        }
    }

    pub fn call(&self, method: &str, params: Option<Value>) -> Result<Value, LxmfError> {
        if !self.is_running() {
            return Err(LxmfError::Io("embedded runtime is stopped".to_string()));
        }

        let request = RpcRequest {
            id: self.inner.next_id.fetch_add(1, Ordering::Relaxed),
            method: method.to_string(),
            params,
        };

        match self.request(RuntimeCommand::Call(request)) {
            Ok(RuntimeResponse::Value(value)) => Ok(value),
            Ok(_) => Err(LxmfError::Io("unexpected runtime response".to_string())),
            Err(err) => {
                self.inner.running.store(false, Ordering::Relaxed);
                Err(err)
            }
        }
    }

    pub fn probe(&self) -> RuntimeProbeReport {
        let local = self.status();
        let started = Instant::now();
        let mut failures = Vec::new();
        let mut rpc_probe = RpcProbeReport {
            reachable: false,
            endpoint: self.inner.settings.rpc.clone(),
            method: None,
            roundtrip_ms: None,
            identity_hash: None,
            status: None,
            errors: Vec::new(),
        };

        if self.is_running() {
            for method in ["daemon_status_ex", "status"] {
                match self.call(method, None) {
                    Ok(status) => {
                        rpc_probe.reachable = true;
                        rpc_probe.method = Some(method.to_string());
                        rpc_probe.roundtrip_ms = Some(started.elapsed().as_millis());
                        rpc_probe.identity_hash = extract_identity_hash(&status);
                        rpc_probe.status = Some(status);
                        rpc_probe.errors = failures.clone();
                        break;
                    }
                    Err(err) => failures.push(format!("{method}: {err}")),
                }
            }
        } else {
            failures.push("runtime not started".to_string());
        }

        if !rpc_probe.reachable {
            rpc_probe.errors = failures;
        }

        let events_started = Instant::now();
        let events_probe = if self.is_running() {
            match self.poll_event() {
                Some(event) => EventsProbeReport {
                    reachable: true,
                    endpoint: self.inner.settings.rpc.clone(),
                    roundtrip_ms: Some(events_started.elapsed().as_millis()),
                    event_type: Some(event.event_type),
                    payload: Some(event.payload),
                    error: None,
                },
                None => EventsProbeReport {
                    reachable: true,
                    endpoint: self.inner.settings.rpc.clone(),
                    roundtrip_ms: Some(events_started.elapsed().as_millis()),
                    event_type: None,
                    payload: None,
                    error: None,
                },
            }
        } else {
            EventsProbeReport {
                reachable: false,
                endpoint: self.inner.settings.rpc.clone(),
                roundtrip_ms: None,
                event_type: None,
                payload: None,
                error: Some("runtime not started".to_string()),
            }
        };

        RuntimeProbeReport {
            profile: self.inner.profile.clone(),
            local,
            rpc: rpc_probe,
            events: events_probe,
        }
    }

    fn request(&self, command: RuntimeCommand) -> Result<RuntimeResponse, LxmfError> {
        let (tx, rx) = std_mpsc::channel();
        self.inner
            .command_tx
            .send(RuntimeRequest { command, respond_to: tx })
            .map_err(|_| LxmfError::Io("embedded runtime worker unavailable".to_string()))?;

        let response = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| LxmfError::Io("embedded runtime worker did not respond".to_string()))?;

        response.map_err(LxmfError::Io)
    }

    fn fallback_status(&self) -> DaemonStatus {
        DaemonStatus {
            running: self.inner.running.load(Ordering::Relaxed),
            pid: None,
            rpc: self.inner.settings.rpc.clone(),
            profile: self.inner.profile.clone(),
            managed: true,
            transport: self.inner.transport.clone(),
            transport_inferred: self.inner.transport_inferred,
            log_path: self.inner.log_path.clone(),
        }
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.stop();
        }
    }
}

pub fn start(config: RuntimeConfig) -> Result<RuntimeHandle, LxmfError> {
    let profile_requested =
        clean_non_empty(Some(config.profile)).unwrap_or_else(|| "default".to_string());
    let profile = resolve_runtime_profile_name(&profile_requested)
        .map_err(|err| LxmfError::Io(err.to_string()))?;
    let mut settings =
        load_profile_settings(&profile).map_err(|err| LxmfError::Io(err.to_string()))?;

    if let Some(rpc) = clean_non_empty(config.rpc) {
        settings.rpc = rpc;
    }
    if let Some(transport) = clean_non_empty(config.transport) {
        settings.transport = Some(transport);
    }

    let paths = profile_paths(&profile).map_err(|err| LxmfError::Io(err.to_string()))?;
    fs::create_dir_all(&paths.root).map_err(|err| LxmfError::Io(err.to_string()))?;

    let config_interfaces =
        load_reticulum_config(&profile).map_err(|err| LxmfError::Io(err.to_string()))?.interfaces;
    let has_enabled_interfaces = config_interfaces.iter().any(|iface| iface.enabled);
    let (transport, transport_inferred) = resolve_transport(&settings, has_enabled_interfaces);

    let (command_tx, command_rx) = unbounded_channel();
    let (startup_tx, startup_rx) = std_mpsc::channel();

    let worker_init = WorkerInit {
        profile: profile.clone(),
        settings: settings.clone(),
        paths: paths.clone(),
        transport: transport.clone(),
        transport_inferred,
        interfaces: config_interfaces,
    };

    thread::Builder::new()
        .name(format!("lxmf-runtime-{}", profile))
        .spawn(move || runtime_thread(worker_init, command_rx, startup_tx))
        .map_err(|err| LxmfError::Io(format!("failed to spawn runtime worker: {err}")))?;

    match startup_rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| LxmfError::Io("runtime startup timed out".to_string()))?
    {
        Ok(()) => {}
        Err(err) => return Err(LxmfError::Io(err)),
    }

    Ok(RuntimeHandle {
        inner: Arc::new(RuntimeInner {
            profile,
            settings,
            running: AtomicBool::new(true),
            next_id: AtomicU64::new(1),
            transport,
            transport_inferred,
            log_path: paths.daemon_log.display().to_string(),
            command_tx,
        }),
    })
}

fn runtime_thread(
    init: WorkerInit,
    command_rx: UnboundedReceiver<RuntimeRequest>,
    startup_tx: std_mpsc::Sender<Result<(), String>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = startup_tx.send(Err(format!("failed to build tokio runtime: {err}")));
            return;
        }
    };

    let local = LocalSet::new();
    local.block_on(&runtime, async move {
        runtime_main(init, command_rx, startup_tx).await;
    });
}

async fn runtime_main(
    init: WorkerInit,
    mut command_rx: UnboundedReceiver<RuntimeRequest>,
    startup_tx: std_mpsc::Sender<Result<(), String>>,
) {
    let mut state = match WorkerState::initialize(init).await {
        Ok(state) => state,
        Err(err) => {
            let _ = startup_tx.send(Err(err.to_string()));
            return;
        }
    };

    let _ = startup_tx.send(Ok(()));

    let mut stopped = false;
    while let Some(request) = command_rx.recv().await {
        let stop_requested = matches!(&request.command, RuntimeCommand::Stop);
        let response = handle_runtime_request(&mut state, request.command);
        let should_exit = matches!(response, Ok(RuntimeResponse::Ack)) && stop_requested;
        if should_exit {
            stopped = true;
        }
        let _ = request.respond_to.send(response);
        if should_exit {
            break;
        }
    }

    if !stopped {
        state.shutdown();
    }
}

fn handle_runtime_request(
    state: &mut WorkerState,
    command: RuntimeCommand,
) -> Result<RuntimeResponse, String> {
    match command {
        RuntimeCommand::Status => {
            let mut status = state.status_template.clone();
            status.running = true;
            Ok(RuntimeResponse::Status(status))
        }
        RuntimeCommand::Call(request) => {
            let response = state
                .daemon
                .handle_rpc(request)
                .map_err(|err| format!("rpc call failed: {err}"))?;
            if let Some(err) = response.error {
                return Err(format!("rpc failed [{}]: {}", err.code, err.message));
            }
            Ok(RuntimeResponse::Value(response.result.unwrap_or(Value::Null)))
        }
        RuntimeCommand::PollEvent => Ok(RuntimeResponse::Event(state.daemon.take_event())),
        RuntimeCommand::Stop => {
            state.shutdown();
            Ok(RuntimeResponse::Ack)
        }
    }
}

impl WorkerState {
    async fn initialize(init: WorkerInit) -> Result<Self, LxmfError> {
        let identity_path = resolve_identity_path(&init.settings, &init.paths);
        drop_empty_identity_stub(&identity_path)?;
        let identity = load_or_create_identity(&identity_path)?;
        let identity_hash = hex::encode(identity.address_hash().as_slice());

        let db_path = init
            .settings
            .db_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| init.paths.daemon_db.clone());
        let store = MessagesStore::open(&db_path).map_err(|err| LxmfError::Io(err.to_string()))?;

        let mut configured_interfaces =
            init.interfaces.iter().cloned().map(interface_to_rpc).collect::<Vec<_>>();

        let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let (receipt_tx, mut receipt_rx) = unbounded_channel();
        let (shutdown_tx, _) = watch::channel(false);

        let mut transport: Option<Arc<Transport>> = None;
        let mut announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>> =
            None;
        let mut delivery_destination_hash_hex: Option<String> = None;
        let mut delivery_source_hash = [0u8; 16];

        if let Some(bind) = init.transport.clone() {
            let mut transport_instance =
                Transport::new(TransportConfig::new("embedded", &identity, true));
            transport_instance
                .set_receipt_handler(Box::new(ReceiptBridge::new(
                    receipt_map.clone(),
                    receipt_tx.clone(),
                )))
                .await;

            let iface_manager = transport_instance.iface_manager();
            iface_manager
                .lock()
                .await
                .spawn(TcpServer::new(bind.clone(), iface_manager.clone()), TcpServer::spawn);

            for entry in &init.interfaces {
                if !entry.enabled || entry.kind != "tcp_client" {
                    continue;
                }
                let Some(host) = entry.host.as_ref() else {
                    continue;
                };
                let Some(port) = entry.port else {
                    continue;
                };
                let addr = format!("{host}:{port}");
                iface_manager.lock().await.spawn(TcpClient::new(addr), TcpClient::spawn);
            }

            if let Some((host, port)) = parse_bind_host_port(&bind) {
                configured_interfaces.push(InterfaceRecord {
                    kind: "tcp_server".into(),
                    enabled: true,
                    host: Some(host),
                    port: Some(port),
                    name: Some("embedded-transport".into()),
                });
            }

            let destination = transport_instance
                .add_destination(identity.clone(), DestinationName::new("lxmf", "delivery"))
                .await;
            {
                let dest = destination.lock().await;
                delivery_source_hash.copy_from_slice(dest.desc.address_hash.as_slice());
                delivery_destination_hash_hex =
                    Some(hex::encode(dest.desc.address_hash.as_slice()));
            }
            announce_destination = Some(destination);
            transport = Some(Arc::new(transport_instance));
        }

        let bridge: Option<Arc<EmbeddedTransportBridge>> = transport
            .as_ref()
            .zip(announce_destination.as_ref())
            .map(|(transport, destination)| {
                Arc::new(EmbeddedTransportBridge::new(
                    transport.clone(),
                    identity.clone(),
                    delivery_source_hash,
                    destination.clone(),
                    init.settings.display_name.as_ref().and_then(|value| {
                        normalize_display_name(value).ok().and_then(|display_name| {
                            encode_delivery_display_name_app_data(&display_name)
                        })
                    }),
                    peer_crypto.clone(),
                    receipt_map.clone(),
                    receipt_tx.clone(),
                ))
            });

        let outbound_bridge: Option<Arc<dyn OutboundBridge>> =
            bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn OutboundBridge>);
        let announce_bridge: Option<Arc<dyn AnnounceBridge>> =
            bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn AnnounceBridge>);

        let daemon = Rc::new(RpcDaemon::with_store_and_bridges(
            store,
            identity_hash,
            outbound_bridge,
            announce_bridge,
        ));

        daemon.set_delivery_destination_hash(delivery_destination_hash_hex);
        daemon.replace_interfaces(configured_interfaces);
        daemon.set_propagation_state(transport.is_some(), None, 0);
        daemon.push_event(RpcEvent {
            event_type: "runtime_started".to_string(),
            payload: json!({ "profile": init.profile }),
        });

        if let Some(bridge) = bridge.as_ref() {
            let _ = bridge.announce_now();
        }

        if transport.is_some() {
            let daemon_receipts = daemon.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::task::spawn_local(async move {
                loop {
                    tokio::select! {
                        changed = shutdown_rx.changed() => {
                            if changed.is_err() || *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        event = receipt_rx.recv() => {
                            let Some(event) = event else {
                                break;
                            };
                            let _ = handle_receipt_event(&daemon_receipts, event);
                        }
                    }
                }
            });
        }

        if let Some(transport) = transport.clone() {
            let daemon_inbound = daemon.clone();
            let inbound_transport = transport.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::task::spawn_local(async move {
                let mut rx = inbound_transport.received_data_events();
                loop {
                    tokio::select! {
                        changed = shutdown_rx.changed() => {
                            if changed.is_err() || *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        result = rx.recv() => {
                            match result {
                                Ok(event) => {
                                    let data = event.data.as_slice();
                                    let mut destination = [0u8; 16];
                                    destination.copy_from_slice(event.destination.as_slice());
                                    if let Some(record) = decode_inbound_payload(destination, data) {
                                        let _ = daemon_inbound.accept_inbound(record);
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            });

            let daemon_announce = daemon.clone();
            let peer_crypto = peer_crypto.clone();
            let announce_transport = transport.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::task::spawn_local(async move {
                let mut rx = announce_transport.recv_announces().await;
                loop {
                    tokio::select! {
                        changed = shutdown_rx.changed() => {
                            if changed.is_err() || *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        result = rx.recv() => {
                            match result {
                                Ok(event) => {
                                    let dest = event.destination.lock().await;
                                    let peer = hex::encode(dest.desc.address_hash.as_slice());
                                    let identity = dest.desc.identity;
                                    let (peer_name, peer_name_source) = parse_peer_name_from_app_data(event.app_data.as_slice())
                                        .map(|(name, source)| (Some(name), Some(source)))
                                        .unwrap_or((None, None));

                                    peer_crypto
                                        .lock()
                                        .expect("peer map")
                                        .insert(peer.clone(), PeerCrypto { identity });

                                    let timestamp = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|value| value.as_secs() as i64)
                                        .unwrap_or(0);

                                    let _ = daemon_announce.accept_announce_with_details(
                                        peer,
                                        timestamp,
                                        peer_name,
                                        peer_name_source,
                                    );
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            });
        }

        let scheduler_handle = if bridge.is_some() {
            Some(daemon.clone().start_announce_scheduler(DEFAULT_ANNOUNCE_INTERVAL_SECS))
        } else {
            None
        };

        Ok(Self {
            profile: init.profile.clone(),
            status_template: DaemonStatus {
                running: true,
                pid: None,
                rpc: init.settings.rpc,
                profile: init.profile,
                managed: true,
                transport: init.transport,
                transport_inferred: init.transport_inferred,
                log_path: init.paths.daemon_log.display().to_string(),
            },
            daemon,
            shutdown_tx,
            scheduler_handle,
            shutdown: false,
        })
    }

    fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.shutdown = true;
        if let Some(handle) = self.scheduler_handle.take() {
            handle.abort();
        }
        let _ = self.shutdown_tx.send(true);
        self.daemon.push_event(RpcEvent {
            event_type: "runtime_stopped".to_string(),
            payload: json!({ "profile": self.profile }),
        });
    }
}

impl EmbeddedTransportBridge {
    #[allow(clippy::too_many_arguments)]
    fn new(
        transport: Arc<Transport>,
        signer: PrivateIdentity,
        delivery_source_hash: [u8; 16],
        announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
        announce_app_data: Option<Vec<u8>>,
        peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
        receipt_map: Arc<Mutex<HashMap<String, String>>>,
        receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self {
            transport,
            signer,
            delivery_source_hash,
            announce_destination,
            announce_app_data,
            peer_crypto,
            receipt_map,
            receipt_tx,
        }
    }
}

impl OutboundBridge for EmbeddedTransportBridge {
    fn deliver(&self, record: &MessageRecord) -> Result<(), std::io::Error> {
        let destination = parse_destination_hex_required(&record.destination)?;
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);

        let payload = build_wire_message(
            self.delivery_source_hash,
            destination,
            &record.title,
            &record.content,
            record.fields.clone(),
            &self.signer,
        )
        .map_err(std::io::Error::other)?;

        let destination_hash = AddressHash::new(destination);
        let transport = self.transport.clone();
        let peer_crypto = self.peer_crypto.clone();
        let receipt_map = self.receipt_map.clone();
        let receipt_tx = self.receipt_tx.clone();
        let message_id = record.id.clone();
        let destination_hex = record.destination.clone();

        tokio::spawn(async move {
            let mut identity = peer_identity;
            transport.request_path(&destination_hash, None, None).await;

            if identity.is_none() {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
                while tokio::time::Instant::now() < deadline {
                    if let Some(found) = transport.destination_identity(&destination_hash).await {
                        identity = Some(found);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }

            let Some(identity) = identity else {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "failed: peer not announced".to_string(),
                });
                return;
            };

            if let Ok(mut peers) = peer_crypto.lock() {
                peers.insert(destination_hex.clone(), PeerCrypto { identity });
            }

            let destination_desc = DestinationDesc {
                identity,
                address_hash: destination_hash,
                name: DestinationName::new("lxmf", "delivery"),
            };

            match send_via_link(
                transport.as_ref(),
                destination_desc,
                payload.as_slice(),
                Duration::from_secs(20),
            )
            .await
            {
                Ok(packet) => {
                    let packet_hash = hex::encode(packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                    let _ = receipt_tx
                        .send(ReceiptEvent { message_id, status: "sent: link".to_string() });
                }
                Err(err) => {
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: format!("link failed: {err}; trying opportunistic"),
                    });

                    let opportunistic_payload =
                        opportunistic_payload(payload.as_slice(), &destination);
                    let mut data = PacketDataBuffer::new();
                    if data.write(opportunistic_payload).is_err() {
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: "failed: payload too large".to_string(),
                        });
                        return;
                    }

                    let packet = Packet {
                        header: Header {
                            ifac_flag: IfacFlag::Open,
                            header_type: HeaderType::Type1,
                            context_flag: ContextFlag::Unset,
                            propagation_type: PropagationType::Broadcast,
                            destination_type: DestinationType::Single,
                            packet_type: PacketType::Data,
                            hops: 0,
                        },
                        ifac: None,
                        destination: destination_hash,
                        transport: None,
                        context: PacketContext::None,
                        data,
                    };

                    let packet_hash = hex::encode(packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                    let trace = transport.send_packet_with_trace(packet).await;
                    if !matches!(
                        trace.outcome,
                        SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast
                    ) {
                        if let Ok(mut map) = receipt_map.lock() {
                            map.remove(&packet_hash);
                        }
                    }

                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id,
                        status: send_outcome_status("opportunistic", trace.outcome),
                    });
                }
            }
        });

        Ok(())
    }
}

impl AnnounceBridge for EmbeddedTransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        let transport = self.transport.clone();
        let destination = self.announce_destination.clone();
        let app_data = self.announce_app_data.clone();
        tokio::spawn(async move {
            transport.send_announce(&destination, app_data.as_deref()).await;
        });
        Ok(())
    }
}

impl ReceiptBridge {
    fn new(
        map: Arc<Mutex<HashMap<String, String>>>,
        tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self { map, tx }
    }
}

impl ReceiptHandler for ReceiptBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let key = hex::encode(receipt.message_id);
        let message_id = self.map.lock().ok().and_then(|mut map| map.remove(&key));
        if let Some(message_id) = message_id {
            let _ = self.tx.send(ReceiptEvent { message_id, status: "delivered".into() });
        }
    }
}

fn handle_receipt_event(daemon: &RpcDaemon, event: ReceiptEvent) -> Result<(), std::io::Error> {
    let _ = daemon.handle_rpc(RpcRequest {
        id: 0,
        method: "record_receipt".into(),
        params: Some(json!({
            "message_id": event.message_id,
            "status": event.status,
        })),
    })?;
    Ok(())
}

fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    if let Ok(mut guard) = map.lock() {
        guard.insert(packet_hash.to_string(), message_id.to_string());
    }
}

async fn send_via_link(
    transport: &Transport,
    destination: DestinationDesc,
    payload: &[u8],
    wait_timeout: Duration,
) -> Result<Packet, std::io::Error> {
    let link = transport.link(destination).await;
    let link_id = *link.lock().await.id();

    if link.lock().await.status() != LinkStatus::Active {
        let mut events = transport.out_link_events();
        let deadline = tokio::time::Instant::now() + wait_timeout;

        loop {
            if link.lock().await.status() == LinkStatus::Active {
                break;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "link activation timed out",
                ));
            }

            // Poll in short slices so we can observe an active link even if the
            // activation event was emitted before we subscribed to link events.
            let wait_slice = remaining.min(Duration::from_millis(250));
            match tokio::time::timeout(wait_slice, events.recv()).await {
                Ok(Ok(event)) => {
                    if event.id == link_id {
                        if let LinkEvent::Activated = event.event {
                            break;
                        }
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "link event channel closed",
                    ));
                }
                Err(_) => continue,
            }
        }
    }

    let packet = link
        .lock()
        .await
        .data_packet(payload)
        .map_err(|err| std::io::Error::other(format!("{err:?}")))?;

    let outcome = transport.send_packet_with_outcome(packet).await;
    if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
        return Err(std::io::Error::other(format!(
            "link packet not sent: {}",
            send_outcome_label(outcome)
        )));
    }

    Ok(packet)
}

fn send_outcome_label(outcome: SendPacketOutcome) -> &'static str {
    match outcome {
        SendPacketOutcome::SentDirect => "sent direct",
        SendPacketOutcome::SentBroadcast => "sent broadcast",
        SendPacketOutcome::DroppedMissingDestinationIdentity => "missing destination identity",
        SendPacketOutcome::DroppedCiphertextTooLarge => "ciphertext too large",
        SendPacketOutcome::DroppedEncryptFailed => "encrypt failed",
        SendPacketOutcome::DroppedNoRoute => "no route",
    }
}

fn send_outcome_status(method: &str, outcome: SendPacketOutcome) -> String {
    match outcome {
        SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast => {
            format!("sent: {method}")
        }
        SendPacketOutcome::DroppedMissingDestinationIdentity => {
            format!("failed: {method} missing destination identity")
        }
        SendPacketOutcome::DroppedCiphertextTooLarge => {
            format!("failed: {method} payload too large")
        }
        SendPacketOutcome::DroppedEncryptFailed => format!("failed: {method} encrypt failed"),
        SendPacketOutcome::DroppedNoRoute => format!("failed: {method} no route"),
    }
}

fn opportunistic_payload<'a>(payload: &'a [u8], destination: &[u8; 16]) -> &'a [u8] {
    if payload.len() > 16 && payload[..16] == destination[..] {
        &payload[16..]
    } else {
        payload
    }
}

fn parse_destination_hex(input: &str) -> Option<[u8; 16]> {
    let bytes = hex::decode(input).ok()?;
    if bytes.len() != 16 {
        return None;
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    Some(out)
}

fn parse_destination_hex_required(input: &str) -> Result<[u8; 16], std::io::Error> {
    parse_destination_hex(input).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid destination hash '{input}' (expected 16-byte hex)"),
        )
    })
}

fn decode_inbound_payload(destination: [u8; 16], payload: &[u8]) -> Option<MessageRecord> {
    let mut decode_candidates = Vec::with_capacity(3);
    decode_candidates.push(payload.to_vec());

    let mut with_destination_prefix = Vec::with_capacity(16 + payload.len());
    with_destination_prefix.extend_from_slice(&destination);
    with_destination_prefix.extend_from_slice(payload);
    decode_candidates.push(with_destination_prefix);

    if payload.len() > 16 && payload[..16] == destination {
        decode_candidates.push(payload[16..].to_vec());
    }

    for candidate in decode_candidates {
        if let Some(record) = decode_wire_candidate(destination, &candidate) {
            return Some(record);
        }
    }
    None
}

fn decode_wire_candidate(
    fallback_destination: [u8; 16],
    candidate: &[u8],
) -> Option<MessageRecord> {
    if let Ok(message) = Message::from_wire(candidate) {
        let source = message.source_hash.unwrap_or([0u8; 16]);
        let destination = message.destination_hash.unwrap_or(fallback_destination);
        let id = wire_message_id_hex(candidate).unwrap_or_else(|| hex::encode(destination));
        return Some(MessageRecord {
            id,
            source: hex::encode(source),
            destination: hex::encode(destination),
            title: String::from_utf8(message.title).unwrap_or_default(),
            content: String::from_utf8(message.content).unwrap_or_default(),
            timestamp: message.timestamp.map(|value| value as i64).unwrap_or(0),
            direction: "in".into(),
            fields: message.fields.as_ref().and_then(rmpv_to_json),
            receipt_status: None,
        });
    }

    let decoded = decode_wire_candidate_relaxed(candidate)?;
    Some(MessageRecord {
        id: decoded.id,
        source: hex::encode(decoded.source),
        destination: hex::encode(decoded.destination),
        title: decoded.title,
        content: decoded.content,
        timestamp: decoded.timestamp,
        direction: "in".into(),
        fields: decoded.fields.as_ref().and_then(rmpv_to_json),
        receipt_status: None,
    })
}

struct RelaxedInboundMessage {
    id: String,
    source: [u8; 16],
    destination: [u8; 16],
    title: String,
    content: String,
    timestamp: i64,
    fields: Option<rmpv::Value>,
}

fn decode_wire_candidate_relaxed(candidate: &[u8]) -> Option<RelaxedInboundMessage> {
    // LXMF wire: 16-byte destination + 16-byte source + 64-byte signature + msgpack payload.
    const SIGNATURE_LEN: usize = 64;
    const HEADER_LEN: usize = 16 + 16 + SIGNATURE_LEN;
    if candidate.len() <= HEADER_LEN {
        return None;
    }

    let mut destination = [0u8; 16];
    destination.copy_from_slice(&candidate[..16]);
    let mut source = [0u8; 16];
    source.copy_from_slice(&candidate[16..32]);
    let payload = &candidate[HEADER_LEN..];
    let payload_value = rmp_serde::from_slice::<rmpv::Value>(payload).ok()?;
    let rmpv::Value::Array(items) = payload_value else {
        return None;
    };
    if items.len() < 4 || items.len() > 5 {
        return None;
    }

    let timestamp = parse_payload_timestamp(items.first()?)? as i64;
    let title = decode_payload_text(items.get(1));
    let content = decode_payload_text(items.get(2));
    let fields = match items.get(3) {
        Some(rmpv::Value::Nil) | None => None,
        Some(value) => Some(value.clone()),
    };

    let payload_without_stamp = payload_without_stamp_bytes(&items)?;
    let id = compute_message_id_hex(destination, source, &payload_without_stamp);

    Some(RelaxedInboundMessage { id, source, destination, title, content, timestamp, fields })
}

fn parse_payload_timestamp(value: &rmpv::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|v| v as f64))
        .or_else(|| value.as_u64().map(|v| v as f64))
}

fn decode_payload_text(value: Option<&rmpv::Value>) -> String {
    match value {
        Some(rmpv::Value::Binary(bytes)) => String::from_utf8(bytes.clone()).unwrap_or_default(),
        Some(rmpv::Value::String(text)) => text.as_str().map(ToOwned::to_owned).unwrap_or_default(),
        _ => String::new(),
    }
}

fn wire_message_id_hex(candidate: &[u8]) -> Option<String> {
    const SIGNATURE_LEN: usize = 64;
    const HEADER_LEN: usize = 16 + 16 + SIGNATURE_LEN;
    if candidate.len() <= HEADER_LEN {
        return None;
    }
    let mut destination = [0u8; 16];
    destination.copy_from_slice(&candidate[..16]);
    let mut source = [0u8; 16];
    source.copy_from_slice(&candidate[16..32]);
    let payload_value = rmp_serde::from_slice::<rmpv::Value>(&candidate[HEADER_LEN..]).ok()?;
    let rmpv::Value::Array(items) = payload_value else {
        return None;
    };
    let payload_without_stamp = payload_without_stamp_bytes(&items)?;
    Some(compute_message_id_hex(destination, source, &payload_without_stamp))
}

fn payload_without_stamp_bytes(items: &[rmpv::Value]) -> Option<Vec<u8>> {
    if items.len() < 4 || items.len() > 5 {
        return None;
    }
    let mut trimmed = items.to_vec();
    if trimmed.len() == 5 {
        trimmed.pop();
    }
    rmp_serde::to_vec(&rmpv::Value::Array(trimmed)).ok()
}

fn compute_message_id_hex(
    destination: [u8; 16],
    source: [u8; 16],
    payload_without_stamp: &[u8],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(destination);
    hasher.update(source);
    hasher.update(payload_without_stamp);
    hex::encode(hasher.finalize())
}

fn build_wire_message(
    source: [u8; 16],
    destination: [u8; 16],
    title: &str,
    content: &str,
    fields: Option<Value>,
    signer: &PrivateIdentity,
) -> Result<Vec<u8>, LxmfError> {
    let mut message = Message::new();
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    if let Some(fields) = fields {
        message.fields = Some(json_to_rmpv(&fields)?);
    }
    message.to_wire(Some(signer))
}

fn json_to_rmpv(value: &Value) -> Result<rmpv::Value, LxmfError> {
    let encoded = rmp_serde::to_vec(value).map_err(|err| LxmfError::Encode(err.to_string()))?;
    let mut cursor = std::io::Cursor::new(encoded);
    rmpv::decode::read_value(&mut cursor).map_err(|err| LxmfError::Decode(err.to_string()))
}

fn rmpv_to_json(value: &rmpv::Value) -> Option<Value> {
    match value {
        rmpv::Value::Nil => Some(Value::Null),
        rmpv::Value::Boolean(v) => Some(Value::Bool(*v)),
        rmpv::Value::Integer(v) => v
            .as_i64()
            .map(|i| Value::Number(i.into()))
            .or_else(|| v.as_u64().map(|u| Value::Number(u.into()))),
        rmpv::Value::F32(v) => serde_json::Number::from_f64(f64::from(*v)).map(Value::Number),
        rmpv::Value::F64(v) => serde_json::Number::from_f64(*v).map(Value::Number),
        rmpv::Value::String(s) => s.as_str().map(|v| Value::String(v.to_string())),
        rmpv::Value::Binary(bytes) => {
            Some(Value::Array(bytes.iter().map(|b| Value::Number((*b).into())).collect()))
        }
        rmpv::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(rmpv_to_json(item)?);
            }
            Some(Value::Array(out))
        }
        rmpv::Value::Map(entries) => {
            let mut object = serde_json::Map::new();
            for (key, value) in entries {
                let key_str = match key {
                    rmpv::Value::String(text) => text.as_str().map(|v| v.to_string()),
                    rmpv::Value::Integer(int) => int
                        .as_i64()
                        .map(|v| v.to_string())
                        .or_else(|| int.as_u64().map(|v| v.to_string())),
                    other => Some(format!("{other:?}")),
                }?;
                object.insert(key_str, rmpv_to_json(value)?);
            }
            Some(Value::Object(object))
        }
        _ => None,
    }
}

fn encode_delivery_display_name_app_data(display_name: &str) -> Option<Vec<u8>> {
    let peer_data = rmpv::Value::Array(vec![
        rmpv::Value::Binary(display_name.as_bytes().to_vec()),
        rmpv::Value::Nil,
    ]);
    rmp_serde::to_vec(&peer_data).ok()
}

fn parse_peer_name_from_app_data(app_data: &[u8]) -> Option<(String, String)> {
    if app_data.is_empty() {
        return None;
    }

    if is_msgpack_array_prefix(app_data[0]) {
        if let Some(name) = display_name_from_app_data(app_data)
            .and_then(|value| normalize_display_name(&value).ok())
        {
            return Some((name, "delivery_app_data".to_string()));
        }
    }

    if let Some(name) = crate::helpers::pn_name_from_app_data(app_data)
        .and_then(|value| normalize_display_name(&value).ok())
    {
        return Some((name, "pn_meta".to_string()));
    }

    let text = std::str::from_utf8(app_data).ok()?;
    let name = normalize_display_name(text).ok()?;
    Some((name, "app_data_utf8".to_string()))
}

fn parse_bind_host_port(bind: &str) -> Option<(String, u16)> {
    if let Ok(addr) = bind.parse::<SocketAddr>() {
        return Some((addr.ip().to_string(), addr.port()));
    }

    let (host, port) = bind.rsplit_once(':')?;
    Some((host.to_string(), port.parse::<u16>().ok()?))
}

fn resolve_transport(
    settings: &ProfileSettings,
    has_enabled_interfaces: bool,
) -> (Option<String>, bool) {
    if let Some(value) = clean_non_empty(settings.transport.clone()) {
        return (Some(value), false);
    }
    if has_enabled_interfaces {
        return (Some(INFERRED_TRANSPORT_BIND.to_string()), true);
    }
    (None, false)
}

fn clean_non_empty(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn interface_to_rpc(entry: InterfaceEntry) -> InterfaceRecord {
    InterfaceRecord {
        kind: entry.kind,
        enabled: entry.enabled,
        host: entry.host,
        port: entry.port,
        name: Some(entry.name),
    }
}

fn extract_identity_hash(status: &Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) = status
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|candidate| !candidate.is_empty())
        {
            return Some(hash.to_string());
        }
    }
    None
}

fn drop_empty_identity_stub(path: &Path) -> Result<(), LxmfError> {
    if let Ok(meta) = fs::metadata(path) {
        if meta.is_file() && meta.len() == 0 {
            fs::remove_file(path).map_err(|err| LxmfError::Io(err.to_string()))?;
        }
    }
    Ok(())
}

fn load_or_create_identity(path: &Path) -> Result<PrivateIdentity, LxmfError> {
    match fs::read(path) {
        Ok(bytes) => {
            return PrivateIdentity::from_private_key_bytes(&bytes)
                .map_err(|err| LxmfError::Io(format!("invalid identity: {err:?}")));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(LxmfError::Io(err.to_string())),
    }

    let identity = PrivateIdentity::new_from_rand(OsRng);
    write_identity_file(path, &identity.to_private_key_bytes())?;
    Ok(identity)
}

fn write_identity_file(path: &Path, key_bytes: &[u8]) -> Result<(), LxmfError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| LxmfError::Io(err.to_string()))?;
        }
    }

    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let tmp_path = path.with_extension(format!("tmp-{unique}"));
    write_private_key_tmp(&tmp_path, key_bytes)?;

    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&tmp_path, path).map_err(|err| LxmfError::Io(err.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|err| LxmfError::Io(err.to_string()))?;
    }

    Ok(())
}

fn write_private_key_tmp(path: &Path, key_bytes: &[u8]) -> Result<(), LxmfError> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(path).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.write_all(key_bytes).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.sync_all().map_err(|err| LxmfError::Io(err.to_string()))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        use std::fs::OpenOptions;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(path).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.write_all(key_bytes).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.sync_all().map_err(|err| LxmfError::Io(err.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::decode_inbound_payload;

    #[test]
    fn decode_inbound_payload_accepts_integer_timestamp_wire() {
        let destination = [0x11; 16];
        let source = [0x22; 16];
        let signature = [0x33; 64];
        let payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
            rmpv::Value::from(1_770_000_000_i64),
            rmpv::Value::from("title"),
            rmpv::Value::from("hello from python-like payload"),
            rmpv::Value::Nil,
        ]))
        .expect("payload encoding");
        let mut wire = Vec::new();
        wire.extend_from_slice(&destination);
        wire.extend_from_slice(&source);
        wire.extend_from_slice(&signature);
        wire.extend_from_slice(&payload);

        let record = decode_inbound_payload(destination, &wire).expect("decoded record");
        assert_eq!(record.source, hex::encode(source));
        assert_eq!(record.destination, hex::encode(destination));
        assert_eq!(record.title, "title");
        assert_eq!(record.content, "hello from python-like payload");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.direction, "in");
    }
}
