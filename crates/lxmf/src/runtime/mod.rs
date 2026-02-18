use crate::cli::daemon::DaemonStatus;
use crate::cli::profile::{
    load_profile_settings, load_reticulum_config, profile_paths, resolve_identity_path,
    resolve_runtime_profile_name, InterfaceEntry, ProfilePaths, ProfileSettings,
};
use crate::helpers::{display_name_from_app_data, is_msgpack_array_prefix, normalize_display_name};
use crate::message::{Message, WireMessage};
use crate::payload_fields::{decode_transport_fields_json, CommandEntry, WireFields};
use crate::LxmfError;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use rand_core::OsRng;
use reticulum::destination::link::{Link, LinkEvent, LinkStatus};
use reticulum::destination::{
    DestinationDesc, DestinationName, SingleInputDestination, SingleOutputDestination,
};
use reticulum::error::RnsError;
use reticulum::hash::{address_hash, AddressHash, Hash};
use reticulum::identity::{Identity, PrivateIdentity};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use reticulum::resource::ResourceEventKind;
use reticulum::rpc::{
    AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon, RpcEvent, RpcRequest,
};
use reticulum::storage::messages::{MessageRecord, MessagesStore};
use reticulum::transport::{
    DeliveryReceipt, ReceiptHandler, SendPacketOutcome, Transport, TransportConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;
use tokio::task::LocalSet;

const INFERRED_TRANSPORT_BIND: &str = "127.0.0.1:0";
const DEFAULT_ANNOUNCE_INTERVAL_SECS: u64 = 60;
const STARTUP_ANNOUNCE_BURST_DELAYS_SECS: &[u64] = &[5, 15, 30];
const POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS: u64 = 20;
const MAX_ALTERNATIVE_PROPAGATION_RELAYS: usize = 3;
const PROPAGATION_PATH_TIMEOUT: Duration = Duration::from_secs(8);
const PROPAGATION_LINK_TIMEOUT: Duration = Duration::from_secs(15);
const PROPAGATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const OUTBOUND_DELIVERY_OPTIONS_FIELD: &str = "__delivery_options";

const PR_IDLE: u32 = 0x00;
const PR_PATH_REQUESTED: u32 = 0x01;
const PR_LINK_ESTABLISHING: u32 = 0x02;
const PR_LINK_ESTABLISHED: u32 = 0x03;
const PR_REQUEST_SENT: u32 = 0x04;
const PR_RECEIVING: u32 = 0x05;
const PR_RESPONSE_RECEIVED: u32 = 0x06;
const PR_COMPLETE: u32 = 0x07;
const PR_NO_PATH: u32 = 0xF0;
const PR_LINK_FAILED: u32 = 0xF1;
const PR_TRANSFER_FAILED: u32 = 0xF2;
const PR_NO_IDENTITY_RCVD: u32 = 0xF3;
const PR_NO_ACCESS: u32 = 0xF4;

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

#[derive(Debug, Clone, Default)]
pub struct SendMessageRequest {
    pub id: Option<String>,
    pub source: Option<String>,
    pub source_private_key: Option<String>,
    pub destination: String,
    pub title: String,
    pub content: String,
    pub fields: Option<Value>,
    pub method: Option<String>,
    pub stamp_cost: Option<u32>,
    pub include_ticket: bool,
    pub try_propagation_on_fail: bool,
}

impl SendMessageRequest {
    pub fn new(destination: impl Into<String>, content: impl Into<String>) -> Self {
        Self { destination: destination.into(), content: content.into(), ..Self::default() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SendCommandRequest {
    pub message: SendMessageRequest,
    pub commands: Vec<CommandEntry>,
}

impl SendCommandRequest {
    pub fn new(
        destination: impl Into<String>,
        content: impl Into<String>,
        commands: Vec<CommandEntry>,
    ) -> Self {
        Self { message: SendMessageRequest::new(destination, content), commands }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SendMessageResponse {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub result: Value,
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

#[derive(Debug)]
struct PreparedSendMessage {
    id: String,
    source: String,
    destination: String,
    params: Value,
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
    transport: Option<Arc<Transport>>,
    local_identity: PrivateIdentity,
    peer_announce_meta: Arc<Mutex<HashMap<String, PeerAnnounceMeta>>>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    peer_identity_cache_path: PathBuf,
    selected_propagation_node: Arc<Mutex<Option<String>>>,
    propagation_sync_state: Arc<Mutex<RuntimePropagationSyncState>>,
    shutdown_tx: watch::Sender<bool>,
    scheduler_handle: Option<tokio::task::JoinHandle<()>>,
    shutdown: bool,
}

#[derive(Debug, Clone)]
struct RuntimePropagationSyncState {
    sync_state: u32,
    state_name: String,
    sync_progress: f64,
    messages_received: u32,
    max_messages: u32,
    selected_node: Option<String>,
    last_sync_started: Option<i64>,
    last_sync_completed: Option<i64>,
    last_sync_error: Option<String>,
}

impl Default for RuntimePropagationSyncState {
    fn default() -> Self {
        Self {
            sync_state: PR_IDLE,
            state_name: "idle".to_string(),
            sync_progress: 0.0,
            messages_received: 0,
            max_messages: 0,
            selected_node: None,
            last_sync_started: None,
            last_sync_completed: None,
            last_sync_error: None,
        }
    }
}

#[derive(Debug, Default)]
struct OutboundDeliveryOptionsCompat {
    method: Option<String>,
    stamp_cost: Option<u32>,
    include_ticket: bool,
    try_propagation_on_fail: bool,
    source_private_key: Option<String>,
    ticket: Option<String>,
}

#[derive(Clone, Copy)]
struct PeerCrypto {
    identity: Identity,
}

#[derive(Clone, Debug, Default)]
struct PeerAnnounceMeta {
    app_data_hex: Option<String>,
}

#[derive(Clone)]
struct AnnounceTarget {
    destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    app_data: Option<Vec<u8>>,
}

struct EmbeddedTransportBridge {
    transport: Arc<Transport>,
    signer: PrivateIdentity,
    delivery_source_hash: [u8; 16],
    announce_targets: Vec<AnnounceTarget>,
    last_announce_epoch_secs: Arc<AtomicU64>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    peer_identity_cache_path: PathBuf,
    selected_propagation_node: Arc<Mutex<Option<String>>>,
    known_propagation_nodes: Arc<Mutex<HashSet<String>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: Arc<Mutex<HashMap<String, String>>>,
    delivered_messages: Arc<Mutex<HashSet<String>>>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

#[derive(Debug, Clone)]
struct ReceiptEvent {
    message_id: String,
    status: String,
}

#[derive(Debug, Deserialize, Default)]
struct RuntimePropagationSyncParams {
    #[serde(default)]
    identity_private_key: Option<String>,
    #[serde(default)]
    max_messages: Option<u32>,
}

fn update_runtime_propagation_sync_state(
    state: &Arc<Mutex<RuntimePropagationSyncState>>,
    update: impl FnOnce(&mut RuntimePropagationSyncState),
) {
    if let Ok(mut guard) = state.lock() {
        update(&mut guard);
    }
}

fn parse_u32_field(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn parse_bool_field(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn parse_string_field(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string())
}

fn merge_outbound_delivery_options(
    api_options: &reticulum::rpc::OutboundDeliveryOptions,
    record: &MessageRecord,
) -> OutboundDeliveryOptionsCompat {
    let mut out = extract_outbound_delivery_options(record);
    if out.method.is_none() {
        out.method = api_options.method.clone();
    }
    if out.stamp_cost.is_none() {
        out.stamp_cost = api_options.stamp_cost;
    }
    out.include_ticket = api_options.include_ticket || out.include_ticket;
    out.try_propagation_on_fail =
        api_options.try_propagation_on_fail || out.try_propagation_on_fail;
    if out.ticket.is_none() {
        out.ticket = api_options.ticket.clone();
    }
    if out.source_private_key.is_none() {
        out.source_private_key = api_options.source_private_key.clone();
    }

    out
}

fn extract_outbound_delivery_options(record: &MessageRecord) -> OutboundDeliveryOptionsCompat {
    let mut out = OutboundDeliveryOptionsCompat::default();
    let Some(fields) = record.fields.as_ref().and_then(Value::as_object) else {
        return out;
    };

    if let Some(options) = fields.get(OUTBOUND_DELIVERY_OPTIONS_FIELD).and_then(Value::as_object) {
        if let Some(method) = parse_string_field(options.get("method").unwrap_or(&Value::Null)) {
            out.method = Some(method);
        }
        if let Some(cost) = parse_u32_field(options.get("stamp_cost").unwrap_or(&Value::Null)) {
            out.stamp_cost = Some(cost);
        }
        if let Some(include_ticket) =
            parse_bool_field(options.get("include_ticket").unwrap_or(&Value::Null))
        {
            out.include_ticket = include_ticket;
        }
        if let Some(try_propagation_on_fail) =
            parse_bool_field(options.get("try_propagation_on_fail").unwrap_or(&Value::Null))
        {
            out.try_propagation_on_fail = try_propagation_on_fail;
        }
        if let Some(ticket) = parse_string_field(options.get("ticket").unwrap_or(&Value::Null)) {
            out.ticket = Some(ticket);
        }
        if let Some(source_private_key) =
            parse_string_field(options.get("source_private_key").unwrap_or(&Value::Null))
        {
            out.source_private_key = Some(source_private_key);
        }
    }

    if let Some(lxmf) = fields
        .get("_lxmf")
        .and_then(Value::as_object)
        .or_else(|| fields.get("lxmf").and_then(Value::as_object))
    {
        if out.method.is_none() {
            if let Some(method) = parse_string_field(lxmf.get("method").unwrap_or(&Value::Null)) {
                out.method = Some(method);
            }
        }
        if out.stamp_cost.is_none() {
            if let Some(cost) = parse_u32_field(lxmf.get("stamp_cost").unwrap_or(&Value::Null)) {
                out.stamp_cost = Some(cost);
            }
        }
        if let Some(include_ticket) =
            parse_bool_field(lxmf.get("include_ticket").unwrap_or(&Value::Null))
        {
            out.include_ticket = include_ticket;
        }
    }

    out
}

#[derive(Clone)]
struct ReceiptBridge {
    map: Arc<Mutex<HashMap<String, String>>>,
    delivered_messages: Arc<Mutex<HashSet<String>>>,
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
                if Self::is_recoverable_rpc_error(&err) {
                    return Err(err);
                }
                self.inner.running.store(false, Ordering::Relaxed);
                Err(err)
            }
        }
    }

    fn is_recoverable_rpc_error(error: &LxmfError) -> bool {
        match error {
            LxmfError::Io(msg) => msg.starts_with("rpc failed ["),
            _ => false,
        }
    }

    pub fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, LxmfError> {
        let source =
            if let Some(source_private_key) = clean_non_empty(request.source_private_key.clone()) {
                source_hash_from_private_key_hex(&source_private_key)?
            } else {
                self.resolve_source_for_send(request.source.clone())?
            };
        let prepared = build_send_params_with_source(request, source)?;
        let PreparedSendMessage { id, source, destination, params } = prepared;

        let has_v2_only_options = params.get("method").is_some()
            || params.get("stamp_cost").is_some()
            || params.get("include_ticket").is_some()
            || params.get("try_propagation_on_fail").is_some()
            || params.get("source_private_key").is_some();
        let result = match self.call("send_message_v2", Some(params.clone())) {
            Ok(value) => value,
            Err(_err) if !has_v2_only_options => self.call("send_message", Some(params))?,
            Err(err) => return Err(err),
        };

        Ok(SendMessageResponse { id, source, destination, result })
    }

    pub fn send_command(
        &self,
        request: SendCommandRequest,
    ) -> Result<SendMessageResponse, LxmfError> {
        if request.commands.is_empty() {
            return Err(LxmfError::Io(
                "send_command requires at least one command entry".to_string(),
            ));
        }
        if request.message.fields.is_some() {
            return Err(LxmfError::Io(
                "send_command does not accept pre-populated fields; use send_message for custom field maps"
                    .to_string(),
            ));
        }

        let mut fields = WireFields::new();
        fields.set_commands(request.commands);

        let mut message = request.message;
        message.fields = Some(fields.to_transport_json()?);
        self.send_message(message)
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

    fn resolve_source_for_send(&self, source: Option<String>) -> Result<String, LxmfError> {
        if let Some(value) = clean_non_empty(source) {
            return Ok(value);
        }

        let mut failures = Vec::new();
        for method in ["daemon_status_ex", "status"] {
            match self.call(method, None) {
                Ok(response) => {
                    if let Some(hash) = extract_identity_hash(&response) {
                        return Ok(hash);
                    }
                    failures.push(format!("{method}: missing identity hash"));
                }
                Err(err) => failures.push(format!("{method}: {err}")),
            }
        }

        let detail =
            if failures.is_empty() { String::new() } else { format!(" ({})", failures.join("; ")) };
        Err(LxmfError::Io(format!(
            "source not provided and daemon did not report delivery/identity hash{detail}"
        )))
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
        let response = handle_runtime_request(&mut state, request.command).await;
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

async fn handle_runtime_request(
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
            let method = request.method.clone();
            let params_snapshot = request.params.clone();
            let mut result = if method == "request_messages_from_propagation_node"
                && state.transport.is_some()
            {
                request_messages_from_propagation_node_live(state, params_snapshot.as_ref())
                    .await
                    .map_err(|err| format!("rpc call failed: {err}"))?
            } else {
                let response = state
                    .daemon
                    .handle_rpc(request)
                    .map_err(|err| format!("rpc call failed: {err}"))?;
                if let Some(err) = response.error {
                    return Err(format!("rpc failed [{}]: {}", err.code, err.message));
                }
                response.result.unwrap_or(Value::Null)
            };
            if method == "list_peers" {
                let snapshot =
                    state.peer_announce_meta.lock().map(|guard| guard.clone()).unwrap_or_default();
                annotate_peer_records_with_announce_metadata(&mut result, &snapshot);
            }
            if method == "set_outbound_propagation_node" {
                let selected = result
                    .get("peer")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                if let Ok(mut guard) = state.selected_propagation_node.lock() {
                    *guard = selected;
                }
            }
            if matches!(
                method.as_str(),
                "store_peer_identity"
                    | "restore_all_peer_identities"
                    | "bulk_restore_peer_identities"
                    | "bulk_restore_announce_identities"
            ) {
                apply_runtime_identity_restore(
                    &state.peer_crypto,
                    &state.peer_identity_cache_path,
                    method.as_str(),
                    params_snapshot.as_ref(),
                );
            }
            annotate_response_meta(&mut result, &state.profile, &state.status_template.rpc);
            Ok(RuntimeResponse::Value(result))
        }
        RuntimeCommand::PollEvent => Ok(RuntimeResponse::Event(state.daemon.take_event())),
        RuntimeCommand::Stop => {
            state.shutdown();
            Ok(RuntimeResponse::Ack)
        }
    }
}

async fn request_messages_from_propagation_node_live(
    state: &WorkerState,
    params: Option<&Value>,
) -> Result<Value, std::io::Error> {
    let parsed = params
        .map(|value| serde_json::from_value::<RuntimePropagationSyncParams>(value.clone()))
        .transpose()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
        .unwrap_or_default();
    let max_messages = parsed.max_messages.unwrap_or(256).clamp(1, 4096);
    let max_messages_usize = max_messages as usize;
    let started_at = now_epoch_secs() as i64;

    let selected_node = state
        .selected_propagation_node
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(selected_node) = selected_node else {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_IDLE;
            guard.state_name = "idle".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.max_messages = max_messages;
            guard.selected_node = None;
            guard.last_sync_started = Some(completed);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some("No propagation node configured".to_string());
        });
        return Ok(json!({
            "success": false,
            "error": "No propagation node configured",
            "errorCode": "NO_PROPAGATION_NODE",
            "state": PR_IDLE,
            "state_name": "idle",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
        }));
    };

    let request_identity = if let Some(raw) =
        parsed.identity_private_key.and_then(|value| clean_non_empty(Some(value)))
    {
        let bytes = hex::decode(raw.trim()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "identity_private_key must be hex-encoded",
            )
        })?;
        PrivateIdentity::from_private_key_bytes(&bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "identity_private_key is not a valid identity private key",
            )
        })?
    } else {
        state.local_identity.clone()
    };

    let Some(transport) = state.transport.clone() else {
        return Err(std::io::Error::other("embedded transport unavailable"));
    };

    let relay_peer = normalize_relay_destination_hash(&state.peer_crypto, &selected_node)
        .unwrap_or(selected_node.clone());
    let Some(relay_destination) = parse_destination_hex(&relay_peer) else {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_NO_PATH;
            guard.state_name = "no_path".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.max_messages = max_messages;
            guard.selected_node = Some(selected_node.clone());
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some("Invalid propagation node hash".to_string());
        });
        return Ok(json!({
            "success": false,
            "error": "Invalid propagation node hash",
            "errorCode": "INVALID_NODE_HASH",
            "state": PR_NO_PATH,
            "state_name": "no_path",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    };
    let relay_hash = AddressHash::new(relay_destination);

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_PATH_REQUESTED;
        guard.state_name = "path_requested".to_string();
        guard.sync_progress = 0.05;
        guard.messages_received = 0;
        guard.max_messages = max_messages;
        guard.selected_node = Some(selected_node.clone());
        guard.last_sync_started = Some(started_at);
        guard.last_sync_completed = None;
        guard.last_sync_error = None;
    });

    let mut relay_identity = transport.destination_identity(&relay_hash).await;
    if relay_identity.is_none() {
        transport.request_path(&relay_hash, None, None).await;
        let deadline = tokio::time::Instant::now() + PROPAGATION_PATH_TIMEOUT;
        while relay_identity.is_none() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            relay_identity = transport.destination_identity(&relay_hash).await;
        }
    }
    let Some(relay_identity) = relay_identity else {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_NO_PATH;
            guard.state_name = "no_path".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.max_messages = max_messages;
            guard.selected_node = Some(selected_node.clone());
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some("No path known for propagation node".to_string());
        });
        return Ok(json!({
            "success": false,
            "error": "No path known for propagation node",
            "errorCode": "NO_PATH",
            "state": PR_NO_PATH,
            "state_name": "no_path",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    };

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_LINK_ESTABLISHING;
        guard.state_name = "link_establishing".to_string();
        guard.sync_progress = 0.2;
    });

    let relay_destination =
        SingleOutputDestination::new(relay_identity, DestinationName::new("lxmf", "propagation"));
    let link = transport.link(relay_destination.desc).await;
    if let Err(err) = await_link_activation(&transport, &link, PROPAGATION_LINK_TIMEOUT).await {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_LINK_FAILED;
            guard.state_name = "link_failed".to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some(err.to_string());
        });
        return Ok(json!({
            "success": false,
            "error": err.to_string(),
            "errorCode": "LINK_FAILED",
            "state": PR_LINK_FAILED,
            "state_name": "link_failed",
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    }

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_LINK_ESTABLISHED;
        guard.state_name = "link_established".to_string();
        guard.sync_progress = 0.35;
    });

    let link_id = *link.lock().await.id();
    let identify_payload = build_link_identify_payload(&request_identity, &link_id);
    send_link_context_packet(
        &transport,
        &link,
        PacketContext::LinkIdentify,
        identify_payload.as_slice(),
    )
    .await?;

    let mut data_rx = transport.received_data_events();
    let mut resource_rx = transport.resource_events();

    let list_payload = build_link_request_payload(
        "/get",
        rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil]),
    )?;
    let list_request_id = send_link_context_packet(
        &transport,
        &link,
        PacketContext::Request,
        list_payload.as_slice(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("missing propagation request id"))?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_REQUEST_SENT;
        guard.state_name = "request_sent".to_string();
        guard.sync_progress = 0.5;
    });

    let list_response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        relay_destination.desc.address_hash,
        link_id,
        list_request_id,
        PROPAGATION_REQUEST_TIMEOUT,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    if let Some((state_code, state_name, message, error_code)) =
        propagation_error_from_response_value(&list_response)
    {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = state_code;
            guard.state_name = state_name.to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some(message.to_string());
        });
        return Ok(json!({
            "success": false,
            "error": message,
            "errorCode": error_code,
            "state": state_code,
            "state_name": state_name,
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    }

    let available_transient_ids = parse_binary_array(&list_response).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid propagation list response payload",
        )
    })?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_RESPONSE_RECEIVED;
        guard.state_name = "response_received".to_string();
        guard.sync_progress = 0.65;
    });

    let wants = available_transient_ids.into_iter().take(max_messages_usize).collect::<Vec<_>>();
    if wants.is_empty() {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = PR_COMPLETE;
            guard.state_name = "complete".to_string();
            guard.sync_progress = 1.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = None;
        });
        return Ok(json!({
            "success": true,
            "state": PR_COMPLETE,
            "state_name": "complete",
            "progress": 1.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
            "last_sync_started": started_at,
            "last_sync_completed": completed,
            "messages": [],
        }));
    }

    let get_payload = build_link_request_payload(
        "/get",
        rmpv::Value::Array(vec![
            rmpv::Value::Array(wants.iter().cloned().map(rmpv::Value::Binary).collect::<Vec<_>>()),
            rmpv::Value::Array(Vec::new()),
            rmpv::Value::Nil,
        ]),
    )?;
    let get_request_id =
        send_link_context_packet(&transport, &link, PacketContext::Request, get_payload.as_slice())
            .await?
            .ok_or_else(|| std::io::Error::other("missing propagation get request id"))?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_REQUEST_SENT;
        guard.state_name = "request_sent".to_string();
        guard.sync_progress = 0.75;
    });

    let get_response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        relay_destination.desc.address_hash,
        link_id,
        get_request_id,
        PROPAGATION_REQUEST_TIMEOUT,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    if let Some((state_code, state_name, message, error_code)) =
        propagation_error_from_response_value(&get_response)
    {
        let completed = now_epoch_secs() as i64;
        update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
            guard.sync_state = state_code;
            guard.state_name = state_name.to_string();
            guard.sync_progress = 0.0;
            guard.messages_received = 0;
            guard.last_sync_started = Some(started_at);
            guard.last_sync_completed = Some(completed);
            guard.last_sync_error = Some(message.to_string());
        });
        return Ok(json!({
            "success": false,
            "error": message,
            "errorCode": error_code,
            "state": state_code,
            "state_name": state_name,
            "progress": 0.0_f64,
            "messages_received": 0_u32,
            "selected_node": selected_node,
            "max_messages": max_messages,
        }));
    }

    let propagation_messages = parse_binary_array(&get_response).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid propagation message response payload",
        )
    })?;

    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_RECEIVING;
        guard.state_name = "receiving".to_string();
        guard.sync_progress = 0.85;
    });

    let mut haves = Vec::new();
    let mut ingested_messages = Vec::new();
    for payload in &propagation_messages {
        let transient_id = Hash::new_from_slice(payload.as_slice()).to_bytes().to_vec();
        haves.push(transient_id.clone());

        if payload.len() >= 16 {
            let mut fallback_destination = [0u8; 16];
            fallback_destination.copy_from_slice(&payload[..16]);
            if let Some(record) = decode_inbound_payload(fallback_destination, payload.as_slice()) {
                state.daemon.accept_inbound(record)?;
                ingested_messages.push(hex::encode(transient_id));
                continue;
            }
        }

        let _ = state.daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "propagation_ingest".to_string(),
            params: Some(json!({
                "transient_id": hex::encode(transient_id),
                "payload_hex": hex::encode(payload),
            })),
        });
    }

    if !haves.is_empty() {
        if let Ok(sync_payload) = build_link_request_payload(
            "/get",
            rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(
                    haves.iter().cloned().map(rmpv::Value::Binary).collect::<Vec<_>>(),
                ),
            ]),
        ) {
            let _ = send_link_context_packet(
                &transport,
                &link,
                PacketContext::Request,
                sync_payload.as_slice(),
            )
            .await;
        }
    }

    let completed = now_epoch_secs() as i64;
    update_runtime_propagation_sync_state(&state.propagation_sync_state, |guard| {
        guard.sync_state = PR_COMPLETE;
        guard.state_name = "complete".to_string();
        guard.sync_progress = 1.0;
        guard.messages_received = u32::try_from(propagation_messages.len()).unwrap_or(u32::MAX);
        guard.last_sync_started = Some(started_at);
        guard.last_sync_completed = Some(completed);
        guard.last_sync_error = None;
    });

    Ok(json!({
        "success": true,
        "state": PR_COMPLETE,
        "state_name": "complete",
        "progress": 1.0_f64,
        "messages_received": propagation_messages.len(),
        "selected_node": selected_node,
        "max_messages": max_messages,
        "last_sync_started": started_at,
        "last_sync_completed": completed,
        "messages": ingested_messages,
    }))
}

fn build_link_identify_payload(identity: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
    let mut public_key = Vec::with_capacity(64);
    public_key.extend_from_slice(identity.as_identity().public_key.as_bytes());
    public_key.extend_from_slice(identity.as_identity().verifying_key.as_bytes());

    let mut signed_data = Vec::with_capacity(16 + public_key.len());
    signed_data.extend_from_slice(link_id.as_slice());
    signed_data.extend_from_slice(public_key.as_slice());
    let signature = identity.sign(signed_data.as_slice());

    let mut payload = Vec::with_capacity(public_key.len() + signature.to_bytes().len());
    payload.extend_from_slice(public_key.as_slice());
    payload.extend_from_slice(signature.to_bytes().as_slice());
    payload
}

fn build_link_request_payload(path: &str, data: rmpv::Value) -> Result<Vec<u8>, std::io::Error> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let path_hash = address_hash(path.as_bytes());
    rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash.to_vec()),
        data,
    ]))
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

async fn send_link_context_packet(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<Link>>,
    context: PacketContext,
    payload: &[u8],
) -> Result<Option<[u8; 16]>, std::io::Error> {
    let packet = {
        let guard = link.lock().await;
        if guard.status() != LinkStatus::Active {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "propagation link is not active",
            ));
        }

        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload, packet_data.accuire_buf_max())
                .map_err(|_| std::io::Error::other("failed to encrypt link packet"))?;
            ciphertext.len()
        };
        packet_data.resize(cipher_len);

        Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: *guard.id(),
            transport: None,
            context,
            data: packet_data,
        }
    };

    let request_id = if context == PacketContext::Request {
        let hash = packet.hash().to_bytes();
        let mut request_id = [0u8; 16];
        request_id.copy_from_slice(&hash[..16]);
        Some(request_id)
    } else {
        None
    };

    let outcome = transport.send_packet_with_outcome(packet).await;
    if !send_outcome_is_sent(outcome) {
        return Err(std::io::Error::other(send_outcome_status("propagation request", outcome)));
    }
    Ok(request_id)
}

async fn wait_for_link_request_response(
    data_rx: &mut tokio::sync::broadcast::Receiver<reticulum::transport::ReceivedData>,
    resource_rx: &mut tokio::sync::broadcast::Receiver<reticulum::resource::ResourceEvent>,
    expected_destination: AddressHash,
    expected_link_id: AddressHash,
    request_id: [u8; 16],
    timeout: Duration,
) -> Result<rmpv::Value, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err("propagation response timed out".to_string());
        }
        let remaining = deadline.saturating_duration_since(now);

        tokio::select! {
            _ = tokio::time::sleep(remaining) => {
                return Err("propagation response timed out".to_string());
            }
            result = data_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.destination != expected_destination {
                            continue;
                        }
                        if let Some((response_id, payload)) = parse_link_response_frame(event.data.as_slice()) {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation response channel closed".to_string());
                    }
                }
            }
            result = resource_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.link_id != expected_link_id {
                            continue;
                        }
                        let reticulum::resource::ResourceEventKind::Complete(complete) = event.kind else {
                            continue;
                        };
                        if let Some((response_id, payload)) = parse_link_response_frame(complete.data.as_slice()) {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation resource channel closed".to_string());
                    }
                }
            }
        }
    }
}

fn parse_link_response_frame(bytes: &[u8]) -> Option<([u8; 16], rmpv::Value)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 2 {
        return None;
    }
    let request_bytes = value_to_bytes(entries.first()?)?;
    if request_bytes.len() != 16 {
        return None;
    }
    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(request_bytes.as_slice());
    Some((request_id, entries.get(1)?.clone()))
}

fn propagation_error_from_response_value(
    value: &rmpv::Value,
) -> Option<(u32, &'static str, &'static str, &'static str)> {
    let code = value.as_u64().or_else(|| value.as_i64().map(|raw| raw as u64))?;
    match code as u32 {
        PR_NO_PATH => {
            Some((PR_NO_PATH, "no_path", "No path known for propagation node", "NO_PATH"))
        }
        PR_LINK_FAILED => {
            Some((PR_LINK_FAILED, "link_failed", "Propagation link failed", "LINK_FAILED"))
        }
        PR_TRANSFER_FAILED => Some((
            PR_TRANSFER_FAILED,
            "transfer_failed",
            "Propagation transfer failed",
            "TRANSFER_FAILED",
        )),
        PR_NO_IDENTITY_RCVD => Some((
            PR_NO_IDENTITY_RCVD,
            "no_identity_rcvd",
            "Propagation node requires identity",
            "NO_IDENTITY_RCVD",
        )),
        PR_NO_ACCESS => {
            Some((PR_NO_ACCESS, "no_access", "Propagation node denied access", "NO_ACCESS"))
        }
        _ => None,
    }
}

fn parse_binary_array(value: &rmpv::Value) -> Option<Vec<Vec<u8>>> {
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let value = value_to_bytes(entry)?;
        out.push(value);
    }
    Some(out)
}

fn value_to_bytes(value: &rmpv::Value) -> Option<Vec<u8>> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.clone()),
        rmpv::Value::String(text) => {
            let value = text.as_str()?;
            if let Ok(decoded) = hex::decode(value) {
                return Some(decoded);
            }
            Some(value.as_bytes().to_vec())
        }
        _ => None,
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

        let peer_identity_cache_path = init.paths.root.join("peer_identities.json");
        let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> =
            Arc::new(Mutex::new(HashMap::new()));
        if let Ok(restored) = load_peer_identity_cache(&peer_identity_cache_path) {
            if let Ok(mut guard) = peer_crypto.lock() {
                *guard = restored;
            }
        }
        let peer_announce_meta: Arc<Mutex<HashMap<String, PeerAnnounceMeta>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let selected_propagation_node: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let known_propagation_nodes: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(HashSet::new()));
        let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let outbound_resource_map: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let delivered_messages: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let last_announce_epoch_secs = Arc::new(AtomicU64::new(0));
        let (receipt_tx, mut receipt_rx) = unbounded_channel();
        let (shutdown_tx, _) = watch::channel(false);

        let mut transport: Option<Arc<Transport>> = None;
        let mut announce_targets: Vec<AnnounceTarget> = Vec::new();
        let mut delivery_destination_hash_hex: Option<String> = None;
        let mut delivery_source_hash = [0u8; 16];
        let normalized_display_name = init
            .settings
            .display_name
            .as_ref()
            .and_then(|value| normalize_display_name(value).ok());

        if let Some(bind) = init.transport.clone() {
            // Embedded desktop runtime should behave as an endpoint, not a transit router.
            // Keep announce/path functionality, but avoid rebroadcasting arbitrary transit traffic.
            let mut transport_instance =
                Transport::new(TransportConfig::new("embedded", &identity, false));
            transport_instance
                .set_receipt_handler(Box::new(ReceiptBridge::new(
                    receipt_map.clone(),
                    delivered_messages.clone(),
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

            let delivery_app_data =
                normalized_display_name.as_deref().and_then(encode_delivery_display_name_app_data);
            announce_targets.push(AnnounceTarget { destination, app_data: delivery_app_data });

            let propagation_destination = transport_instance
                .add_destination(identity.clone(), DestinationName::new("lxmf", "propagation"))
                .await;
            let propagation_app_data =
                encode_propagation_node_app_data(normalized_display_name.as_deref());
            announce_targets.push(AnnounceTarget {
                destination: propagation_destination,
                app_data: propagation_app_data,
            });
            transport = Some(Arc::new(transport_instance));
        }

        let bridge: Option<Arc<EmbeddedTransportBridge>> = transport.as_ref().map(|transport| {
            Arc::new(EmbeddedTransportBridge::new(
                transport.clone(),
                identity.clone(),
                delivery_source_hash,
                announce_targets.clone(),
                last_announce_epoch_secs.clone(),
                peer_crypto.clone(),
                peer_identity_cache_path.clone(),
                selected_propagation_node.clone(),
                known_propagation_nodes.clone(),
                receipt_map.clone(),
                outbound_resource_map.clone(),
                delivered_messages.clone(),
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
        let propagation_enabled = bridge.is_some();
        daemon.set_propagation_state(propagation_enabled, None, crate::constants::PROPAGATION_COST);
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
                                    if let Some(mut record) = decode_inbound_payload(destination, data) {
                                        annotate_inbound_transport_metadata(&mut record, &event);
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

            let daemon_resource_inbound = daemon.clone();
            let resource_transport = transport.clone();
            let resource_receipt_tx = receipt_tx.clone();
            let resource_outbound_map = outbound_resource_map.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::task::spawn_local(async move {
                let mut rx = resource_transport.resource_events();
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
                                    match event.kind {
                                        ResourceEventKind::Complete(complete) => {
                                            if let Some(destination) = resolve_link_destination(&resource_transport, &event.link_id).await {
                                                if let Some(record) = decode_inbound_payload(destination, &complete.data) {
                                                    let _ = daemon_resource_inbound.accept_inbound(record);
                                                }
                                            }
                                        }
                                        ResourceEventKind::OutboundComplete => {
                                            let resource_hash_hex = hex::encode(event.hash.as_slice());
                                            let message_id = resource_outbound_map
                                                .lock()
                                                .ok()
                                                .and_then(|mut guard| guard.remove(&resource_hash_hex));
                                            if let Some(message_id) = message_id {
                                                let _ = resource_receipt_tx.send(ReceiptEvent {
                                                    message_id,
                                                    status: "sent: link resource".to_string(),
                                                });
                                            }
                                        }
                                        ResourceEventKind::Progress(_) => {}
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
            let peer_announce_meta = peer_announce_meta.clone();
            let peer_identity_cache_path = peer_identity_cache_path.clone();
            let known_propagation_nodes = known_propagation_nodes.clone();
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
                                    let app_data = event.app_data.as_slice();
                                    let (peer_name, peer_name_source) = parse_peer_name_from_app_data(app_data)
                                        .map(|(name, source)| (Some(name), Some(source)))
                                        .unwrap_or((None, None));

                                    peer_crypto
                                        .lock()
                                        .expect("peer map")
                                        .insert(peer.clone(), PeerCrypto { identity });
                                    persist_peer_identity_cache(
                                        &peer_crypto,
                                        &peer_identity_cache_path,
                                    );
                                    update_peer_announce_meta(
                                        &peer_announce_meta,
                                        &peer,
                                        app_data,
                                    );

                                    let timestamp = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|value| value.as_secs() as i64)
                                        .unwrap_or(0);
                                    let app_data_hex = (!app_data.is_empty()).then(|| hex::encode(app_data));
                                    let aspect =
                                        lxmf_aspect_from_name_hash(dest.desc.name.as_name_hash_slice());
                                    if aspect.as_deref() == Some("lxmf.propagation") {
                                        if let Ok(mut nodes) = known_propagation_nodes.lock() {
                                            nodes.insert(peer.clone());
                                        }
                                    }
                                    let hops = Some(u32::from(event.hops));
                                    let interface =
                                        Some(hex::encode(event.interface.as_slice()));

                                    let _ = daemon_announce.accept_announce_with_metadata(
                                        peer,
                                        timestamp,
                                        peer_name,
                                        peer_name_source,
                                        app_data_hex,
                                        None,
                                        None,
                                        None,
                                        None,
                                        aspect,
                                        hops,
                                        interface,
                                        None,
                                        None,
                                        None,
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

        if let Some(bridge) = bridge.clone() {
            tokio::task::spawn_local(async move {
                // Emit a short announce burst after startup to improve cross-client
                // discovery when peers/interfaces come online slightly later.
                for delay_secs in STARTUP_ANNOUNCE_BURST_DELAYS_SECS {
                    tokio::time::sleep(Duration::from_secs(*delay_secs)).await;
                    let _ = bridge.announce_now();
                }
            });
        }

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
            transport,
            local_identity: identity,
            peer_announce_meta,
            peer_crypto,
            peer_identity_cache_path,
            selected_propagation_node,
            propagation_sync_state: Arc::new(Mutex::new(RuntimePropagationSyncState::default())),
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
        announce_targets: Vec<AnnounceTarget>,
        last_announce_epoch_secs: Arc<AtomicU64>,
        peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
        peer_identity_cache_path: PathBuf,
        selected_propagation_node: Arc<Mutex<Option<String>>>,
        known_propagation_nodes: Arc<Mutex<HashSet<String>>>,
        receipt_map: Arc<Mutex<HashMap<String, String>>>,
        outbound_resource_map: Arc<Mutex<HashMap<String, String>>>,
        delivered_messages: Arc<Mutex<HashSet<String>>>,
        receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self {
            transport,
            signer,
            delivery_source_hash,
            announce_targets,
            last_announce_epoch_secs,
            peer_crypto,
            peer_identity_cache_path,
            selected_propagation_node,
            known_propagation_nodes,
            receipt_map,
            outbound_resource_map,
            delivered_messages,
            receipt_tx,
        }
    }
}

impl OutboundBridge for EmbeddedTransportBridge {
    fn deliver(
        &self,
        record: &MessageRecord,
        options: &reticulum::rpc::OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let destination = parse_destination_hex_required(&record.destination)?;
        let options = merge_outbound_delivery_options(options, record);
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);
        let (signer, source_hash) = if let Some(source_private_key) =
            clean_non_empty(options.source_private_key)
        {
            let source_key_bytes = hex::decode(source_private_key.trim()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "source_private_key must be hex-encoded",
                )
            })?;
            let signer =
                PrivateIdentity::from_private_key_bytes(&source_key_bytes).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "source_private_key is not a valid identity private key",
                    )
                })?;
            let mut source_hash = [0u8; 16];
            source_hash.copy_from_slice(signer.address_hash().as_slice());
            (signer, source_hash)
        } else {
            if let Some(requested_source) = parse_destination_hex(&record.source) {
                if requested_source != self.delivery_source_hash {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "source hash differs from runtime identity; set source_private_key for per-message source identities",
                    ));
                }
            }
            (self.signer.clone(), self.delivery_source_hash)
        };
        let outbound_fields = sanitize_outbound_wire_fields(record.fields.as_ref());

        let payload = build_wire_message(
            source_hash,
            destination,
            &record.title,
            &record.content,
            outbound_fields.clone(),
            &signer,
        )
        .map_err(std::io::Error::other)?;
        let requested_method = parse_delivery_method(options.method.as_deref());
        let opportunistic_supported = can_send_opportunistic(
            outbound_fields.as_ref(),
            opportunistic_payload(payload.as_slice(), &destination).len(),
        );
        let mut effective_method = requested_method;
        if matches!(requested_method, DeliveryMethod::Opportunistic) && !opportunistic_supported {
            effective_method = DeliveryMethod::Direct;
        }

        let destination_hash = AddressHash::new(destination);
        let transport = self.transport.clone();
        let peer_crypto = self.peer_crypto.clone();
        let selected_propagation_node = self.selected_propagation_node.clone();
        let known_propagation_nodes = self.known_propagation_nodes.clone();
        let receipt_map = self.receipt_map.clone();
        let outbound_resource_map = self.outbound_resource_map.clone();
        let delivered_messages = self.delivered_messages.clone();
        let receipt_tx = self.receipt_tx.clone();
        let announce_targets = self.announce_targets.clone();
        let announce_last = self.last_announce_epoch_secs.clone();
        let peer_identity_cache_path = self.peer_identity_cache_path.clone();
        let message_id = record.id.clone();
        let destination_hex = record.destination.clone();
        let try_propagation_on_fail = options.try_propagation_on_fail;
        let include_ticket = options.include_ticket;
        let ticket_present =
            options.ticket.as_ref().map(|ticket| !ticket.trim().is_empty()).unwrap_or(false);

        tokio::spawn(async move {
            if let Ok(mut delivered) = delivered_messages.lock() {
                delivered.remove(&message_id);
            }

            if include_ticket {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: if ticket_present {
                        "ticket: present".to_string()
                    } else {
                        "ticket: unavailable".to_string()
                    },
                });
            }

            if !matches!(requested_method, DeliveryMethod::Auto)
                && requested_method != effective_method
            {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: "retrying: direct fallback due to opportunistic constraints"
                        .to_string(),
                });
            }

            let allow_link =
                matches!(effective_method, DeliveryMethod::Auto | DeliveryMethod::Direct);
            let allow_opportunistic =
                matches!(effective_method, DeliveryMethod::Auto | DeliveryMethod::Opportunistic);
            let allow_propagated =
                matches!(effective_method, DeliveryMethod::Auto | DeliveryMethod::Propagated)
                    || try_propagation_on_fail;
            let mut last_failure: Option<String> = None;

            if allow_link {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: "outbound_attempt: link".to_string(),
                });

                let mut identity = peer_identity;
                transport.request_path(&destination_hash, None, None).await;

                if identity.is_none() {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
                    while tokio::time::Instant::now() < deadline {
                        if let Some(found) = transport.destination_identity(&destination_hash).await
                        {
                            identity = Some(found);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }

                if let Some(identity) = identity {
                    if let Ok(mut peers) = peer_crypto.lock() {
                        peers.insert(destination_hex.clone(), PeerCrypto { identity });
                    }
                    persist_peer_identity_cache(&peer_crypto, &peer_identity_cache_path);

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
                        Ok(LinkSendResult::Packet(packet)) => {
                            let packet_hash = hex::encode(packet.hash().to_bytes());
                            track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                            trigger_rate_limited_announce(
                                &transport,
                                &announce_targets,
                                &announce_last,
                                POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                            );
                            let _ = receipt_tx.send(ReceiptEvent {
                                message_id,
                                status: "sent: link".to_string(),
                            });
                            return;
                        }
                        Ok(LinkSendResult::Resource(resource_hash)) => {
                            track_outbound_resource_mapping(
                                &outbound_resource_map,
                                &resource_hash,
                                &message_id,
                            );
                            trigger_rate_limited_announce(
                                &transport,
                                &announce_targets,
                                &announce_last,
                                POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                            );
                            let _ = receipt_tx.send(ReceiptEvent {
                                message_id,
                                status: "sending: link resource".to_string(),
                            });
                            return;
                        }
                        Err(err) => {
                            last_failure = Some(format!("failed: link {err}"));
                        }
                    }
                } else {
                    last_failure = Some("failed: peer not announced".to_string());
                }

                if !allow_opportunistic && !allow_propagated {
                    prune_receipt_mappings_for_message(&receipt_map, &message_id);
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id,
                        status: last_failure
                            .unwrap_or_else(|| "failed: link delivery unavailable".to_string()),
                    });
                    return;
                }
            }

            if allow_opportunistic {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: "outbound_attempt: opportunistic".to_string(),
                });
                let opportunistic_payload = opportunistic_payload(payload.as_slice(), &destination);
                let mut opportunistic_data = PacketDataBuffer::new();
                if opportunistic_data.write(opportunistic_payload).is_ok()
                    && opportunistic_supported
                {
                    let opportunistic_packet = Packet {
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
                        data: opportunistic_data,
                    };
                    let opportunistic_hash = hex::encode(opportunistic_packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &opportunistic_hash, &message_id);
                    let opportunistic_trace =
                        transport.send_packet_with_trace(opportunistic_packet).await;
                    if !send_outcome_is_sent(opportunistic_trace.outcome) {
                        if let Ok(mut map) = receipt_map.lock() {
                            map.remove(&opportunistic_hash);
                        }
                        let failed =
                            send_outcome_status("opportunistic", opportunistic_trace.outcome);
                        last_failure = Some(failed.clone());
                        if !allow_propagated {
                            let _ = receipt_tx.send(ReceiptEvent { message_id, status: failed });
                            return;
                        }
                    } else {
                        trigger_rate_limited_announce(
                            &transport,
                            &announce_targets,
                            &announce_last,
                            POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                        );
                        let status =
                            send_outcome_status("opportunistic", opportunistic_trace.outcome);
                        let _ = receipt_tx
                            .send(ReceiptEvent { message_id: message_id.clone(), status });
                        if !allow_propagated {
                            return;
                        }

                        tokio::time::sleep(Duration::from_secs(20)).await;
                        if is_message_marked_delivered(&delivered_messages, &message_id) {
                            return;
                        }
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id: message_id.clone(),
                            status: "retrying: propagated relay after opportunistic timeout"
                                .to_string(),
                        });
                    }
                } else {
                    last_failure =
                        Some("failed: opportunistic payload too large or unsupported".to_string());
                    if !allow_propagated {
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: last_failure
                                .unwrap_or_else(|| "failed: opportunistic unavailable".to_string()),
                        });
                        return;
                    }
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: "retrying: propagated relay after opportunistic constraints"
                            .to_string(),
                    });
                }
            }

            if !allow_propagated {
                prune_receipt_mappings_for_message(&receipt_map, &message_id);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: last_failure
                        .unwrap_or_else(|| "failed: delivery unavailable".to_string()),
                });
                return;
            }

            let mut destination_identity = peer_identity;
            if destination_identity.is_none() {
                transport.request_path(&destination_hash, None, None).await;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                while tokio::time::Instant::now() < deadline {
                    if let Some(found) = transport.destination_identity(&destination_hash).await {
                        destination_identity = Some(found);
                        if let Ok(mut peers) = peer_crypto.lock() {
                            peers.insert(destination_hex.clone(), PeerCrypto { identity: found });
                        }
                        persist_peer_identity_cache(&peer_crypto, &peer_identity_cache_path);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }

            let Some(destination_identity) = destination_identity else {
                prune_receipt_mappings_for_message(&receipt_map, &message_id);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "failed: propagated relay missing destination identity".to_string(),
                });
                return;
            };

            let propagated_payload =
                match build_propagation_envelope(payload.as_slice(), &destination_identity) {
                    Ok(encoded) => encoded,
                    Err(err) => {
                        prune_receipt_mappings_for_message(&receipt_map, &message_id);
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: format!("failed: propagated relay encoding error ({err})"),
                        });
                        return;
                    }
                };

            let mut relay_candidates =
                propagation_relay_candidates(&selected_propagation_node, &known_propagation_nodes)
                    .into_iter()
                    .take(MAX_ALTERNATIVE_PROPAGATION_RELAYS)
                    .collect::<Vec<_>>();
            if relay_candidates.is_empty() {
                prune_receipt_mappings_for_message(&receipt_map, &message_id);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "failed: no propagation relay selected".to_string(),
                });
                return;
            }

            let mut last_relay_failure =
                last_failure.unwrap_or_else(|| "failed: propagated relay unavailable".to_string());
            let mut attempted_relays: Vec<String> = Vec::new();
            let mut candidate_idx = 0usize;
            while candidate_idx < relay_candidates.len() {
                let relay_candidate = relay_candidates[candidate_idx].clone();
                candidate_idx += 1;
                let relay_peer = normalize_relay_destination_hash(&peer_crypto, &relay_candidate)
                    .unwrap_or(relay_candidate.clone());
                if !attempted_relays.iter().any(|entry| entry == &relay_peer) {
                    attempted_relays.push(relay_peer.clone());
                }
                let Some(relay_destination) = parse_destination_hex(&relay_peer) else {
                    last_relay_failure =
                        format!("failed: invalid propagation relay hash '{relay_peer}'");
                    continue;
                };
                let relay_hash = AddressHash::new(relay_destination);
                transport.request_path(&relay_hash, None, None).await;
                let relay_known_deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                let mut relay_known = transport.destination_identity(&relay_hash).await.is_some();
                while !relay_known && tokio::time::Instant::now() < relay_known_deadline {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    relay_known = transport.destination_identity(&relay_hash).await.is_some();
                }
                if !relay_known {
                    last_relay_failure = "failed: propagation relay not announced".to_string();
                    if candidate_idx < relay_candidates.len() {
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id: message_id.clone(),
                            status: format_relay_request_status(attempted_relays.as_slice()),
                        });
                    } else if let Some(external_relay) = wait_for_external_relay_selection(
                        &selected_propagation_node,
                        &peer_crypto,
                        attempted_relays.as_slice(),
                        Duration::from_secs(8),
                    )
                    .await
                    {
                        relay_candidates.push(external_relay);
                    }
                    continue;
                }

                for attempt in 1..=2u8 {
                    if is_message_marked_delivered(&delivered_messages, &message_id) {
                        return;
                    }
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: format!(
                            "retrying: propagated relay attempt {attempt}/2 via {}",
                            short_hash_prefix(&relay_peer)
                        ),
                    });

                    let mut relay_data = PacketDataBuffer::new();
                    if relay_data.write(propagated_payload.as_slice()).is_err() {
                        prune_receipt_mappings_for_message(&receipt_map, &message_id);
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: "failed: propagated relay payload too large".to_string(),
                        });
                        return;
                    }
                    let relay_packet = Packet {
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
                        destination: relay_hash,
                        transport: None,
                        context: PacketContext::None,
                        data: relay_data,
                    };
                    let relay_packet_hash = hex::encode(relay_packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &relay_packet_hash, &message_id);
                    let relay_trace = transport.send_packet_with_trace(relay_packet).await;
                    if send_outcome_is_sent(relay_trace.outcome) {
                        trigger_rate_limited_announce(
                            &transport,
                            &announce_targets,
                            &announce_last,
                            POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                        );
                        if let Ok(mut selected) = selected_propagation_node.lock() {
                            *selected = Some(relay_peer.clone());
                        }
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: send_outcome_status("propagated relay", relay_trace.outcome),
                        });
                        return;
                    }
                    if let Ok(mut map) = receipt_map.lock() {
                        map.remove(&relay_packet_hash);
                    }
                    last_relay_failure =
                        send_outcome_status("propagated relay", relay_trace.outcome);
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }

                if candidate_idx < relay_candidates.len() {
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: format_relay_request_status(attempted_relays.as_slice()),
                    });
                } else if let Some(external_relay) = wait_for_external_relay_selection(
                    &selected_propagation_node,
                    &peer_crypto,
                    attempted_relays.as_slice(),
                    Duration::from_secs(8),
                )
                .await
                {
                    relay_candidates.push(external_relay);
                }
            }

            if !attempted_relays.is_empty() {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: format_relay_request_status(attempted_relays.as_slice()),
                });
            }
            prune_receipt_mappings_for_message(&receipt_map, &message_id);
            let _ = receipt_tx.send(ReceiptEvent { message_id, status: last_relay_failure });
        });

        Ok(())
    }
}

impl AnnounceBridge for EmbeddedTransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        self.last_announce_epoch_secs.store(now_epoch_secs(), Ordering::Relaxed);
        let transport = self.transport.clone();
        let announce_targets = self.announce_targets.clone();
        tokio::spawn(async move {
            for target in announce_targets {
                transport.send_announce(&target.destination, target.app_data.as_deref()).await;
            }
        });
        Ok(())
    }
}

impl ReceiptBridge {
    fn new(
        map: Arc<Mutex<HashMap<String, String>>>,
        delivered_messages: Arc<Mutex<HashSet<String>>>,
        tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self { map, delivered_messages, tx }
    }
}

impl ReceiptHandler for ReceiptBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let key = hex::encode(receipt.message_id);
        let message_id = self.map.lock().ok().and_then(|mut map| map.remove(&key));
        if let Some(message_id) = message_id {
            if let Ok(mut delivered) = self.delivered_messages.lock() {
                delivered.insert(message_id.clone());
            }
            let _ = self.tx.send(ReceiptEvent { message_id, status: "delivered".into() });
        }
    }
}

fn handle_receipt_event(daemon: &RpcDaemon, event: ReceiptEvent) -> Result<(), std::io::Error> {
    let message_id = event.message_id;
    let status = event.status;
    let _ = daemon.handle_rpc(RpcRequest {
        id: 0,
        method: "record_receipt".into(),
        params: Some(json!({
            "message_id": message_id,
            "status": status,
        })),
    })?;
    if let Some(exclude_relays) = parse_alternative_relay_request_status(status.as_str()) {
        daemon.push_event(RpcEvent {
            event_type: "alternative_relay_request".to_string(),
            payload: json!({
                "message_id": message_id,
                "exclude_relays": exclude_relays,
                "timestamp_ms": (now_epoch_secs() as i64) * 1000,
            }),
        });
    }
    Ok(())
}

fn format_relay_request_status(exclude_relays: &[String]) -> String {
    if exclude_relays.is_empty() {
        return "retrying: requesting alternative propagation relay".to_string();
    }
    format!(
        "retrying: requesting alternative propagation relay;exclude={}",
        exclude_relays.join(",")
    )
}

async fn wait_for_external_relay_selection(
    selected_propagation_node: &Arc<Mutex<Option<String>>>,
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    attempted_relays: &[String],
    timeout: Duration,
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let selected = selected_propagation_node
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(selected) = selected {
            let normalized =
                normalize_relay_destination_hash(peer_crypto, &selected).unwrap_or(selected);
            if !attempted_relays.iter().any(|relay| relay == &normalized) {
                return Some(normalized);
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    None
}

fn parse_alternative_relay_request_status(status: &str) -> Option<Vec<String>> {
    const PREFIX: &str = "retrying: requesting alternative propagation relay";
    if !status.starts_with(PREFIX) {
        return None;
    }
    let excludes_raw = status.split(";exclude=").nth(1).unwrap_or_default();
    let exclude_relays = excludes_raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Some(exclude_relays)
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

fn track_outbound_resource_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    resource_hash: &reticulum::hash::Hash,
    message_id: &str,
) {
    if let Ok(mut guard) = map.lock() {
        guard.insert(hex::encode(resource_hash.as_slice()), message_id.to_string());
    }
}

async fn send_via_link(
    transport: &Transport,
    destination: DestinationDesc,
    payload: &[u8],
    wait_timeout: Duration,
) -> Result<LinkSendResult, std::io::Error> {
    let link = transport.link(destination).await;
    await_link_activation(transport, &link, wait_timeout).await?;
    let link_id = *link.lock().await.id();

    let packet = {
        let guard = link.lock().await;
        guard.data_packet(payload)
    };
    match packet {
        Ok(packet) => {
            let outcome = transport.send_packet_with_outcome(packet).await;
            if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast)
            {
                return Err(std::io::Error::other(format!(
                    "link packet not sent: {}",
                    send_outcome_label(outcome)
                )));
            }
            Ok(LinkSendResult::Packet(Box::new(packet)))
        }
        Err(RnsError::OutOfMemory | RnsError::InvalidArgument) => {
            let resource_hash = transport
                .send_resource(&link_id, payload.to_vec(), None)
                .await
                .map_err(|err| std::io::Error::other(format!("link resource not sent: {err:?}")))?;
            Ok(LinkSendResult::Resource(resource_hash))
        }
        Err(err) => Err(std::io::Error::other(format!("{err:?}"))),
    }
}

async fn await_link_activation(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<Link>>,
    wait_timeout: Duration,
) -> Result<(), std::io::Error> {
    let link_id = *link.lock().await.id();
    if link.lock().await.status() == LinkStatus::Active {
        return Ok(());
    }

    let mut events = transport.out_link_events();
    let deadline = tokio::time::Instant::now() + wait_timeout;

    loop {
        if link.lock().await.status() == LinkStatus::Active {
            return Ok(());
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
                        return Ok(());
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

enum LinkSendResult {
    Packet(Box<Packet>),
    Resource(reticulum::hash::Hash),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryMethod {
    Auto,
    Direct,
    Opportunistic,
    Propagated,
}

fn parse_delivery_method(method: Option<&str>) -> DeliveryMethod {
    let Some(method) = method.map(str::trim).filter(|value| !value.is_empty()) else {
        return DeliveryMethod::Auto;
    };

    match method.to_ascii_lowercase().as_str() {
        "direct" | "link" => DeliveryMethod::Direct,
        "opportunistic" => DeliveryMethod::Opportunistic,
        "propagated" | "propagation" | "relay" => DeliveryMethod::Propagated,
        _ => DeliveryMethod::Auto,
    }
}

fn can_send_opportunistic(fields: Option<&Value>, payload_len: usize) -> bool {
    const MAX_OPPORTUNISTIC_BYTES: usize = 295;
    payload_len <= MAX_OPPORTUNISTIC_BYTES && !fields_contain_attachments(fields)
}

fn fields_contain_attachments(fields: Option<&Value>) -> bool {
    let Some(fields) = fields else {
        return false;
    };
    let Some(map) = fields.as_object() else {
        return false;
    };

    for key in ["5", "attachments", "files"] {
        if map.get(key).is_some() {
            return true;
        }
    }
    false
}

async fn resolve_link_destination(
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<[u8; 16]> {
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        let mut destination = [0u8; 16];
        destination.copy_from_slice(guard.destination().address_hash.as_slice());
        return Some(destination);
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        let mut destination = [0u8; 16];
        destination.copy_from_slice(guard.destination().address_hash.as_slice());
        return Some(destination);
    }
    None
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

fn send_outcome_is_sent(outcome: SendPacketOutcome) -> bool {
    matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast)
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

fn is_message_marked_delivered(
    delivered_messages: &Arc<Mutex<HashSet<String>>>,
    message_id: &str,
) -> bool {
    delivered_messages.lock().map(|guard| guard.contains(message_id)).unwrap_or(false)
}

fn prune_receipt_mappings_for_message(
    receipt_map: &Arc<Mutex<HashMap<String, String>>>,
    message_id: &str,
) {
    if let Ok(mut guard) = receipt_map.lock() {
        guard.retain(|_, mapped_message_id| mapped_message_id != message_id);
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn try_acquire_announce_window(
    last_announce_epoch_secs: &Arc<AtomicU64>,
    min_interval_secs: u64,
) -> bool {
    let now = now_epoch_secs();
    loop {
        let previous = last_announce_epoch_secs.load(Ordering::Relaxed);
        if previous != 0 && now.saturating_sub(previous) < min_interval_secs {
            return false;
        }
        if last_announce_epoch_secs
            .compare_exchange(previous, now, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return true;
        }
    }
}

fn trigger_rate_limited_announce(
    transport: &Arc<Transport>,
    announce_targets: &[AnnounceTarget],
    last_announce_epoch_secs: &Arc<AtomicU64>,
    min_interval_secs: u64,
) {
    if !try_acquire_announce_window(last_announce_epoch_secs, min_interval_secs) {
        return;
    }
    let announce_transport = transport.clone();
    let announce_targets = announce_targets.to_vec();
    tokio::spawn(async move {
        for target in announce_targets {
            announce_transport.send_announce(&target.destination, target.app_data.as_deref()).await;
        }
    });
}

fn opportunistic_payload<'a>(payload: &'a [u8], destination: &[u8; 16]) -> &'a [u8] {
    if payload.len() > 16 && payload[..16] == destination[..] {
        &payload[16..]
    } else {
        payload
    }
}

fn build_propagation_envelope(
    wire_payload: &[u8],
    destination_identity: &Identity,
) -> Result<Vec<u8>, String> {
    let wire = WireMessage::unpack(wire_payload).map_err(|err: LxmfError| err.to_string())?;
    wire.pack_propagation_with_rng(destination_identity, now_epoch_secs() as f64, OsRng)
        .map_err(|err: LxmfError| err.to_string())
}

fn propagation_relay_candidates(
    selected_propagation_node: &Arc<Mutex<Option<String>>>,
    known_propagation_nodes: &Arc<Mutex<HashSet<String>>>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    let selected = selected_propagation_node
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(selected) = selected {
        seen.insert(selected.clone());
        candidates.push(selected);
    }

    let mut known = known_propagation_nodes
        .lock()
        .map(|guard| guard.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    known.sort();
    for candidate in known {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }

    candidates
}

fn short_hash_prefix(value: &str) -> String {
    value.chars().take(12).collect::<String>()
}

fn normalize_relay_destination_hash(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    selected_hash: &str,
) -> Option<String> {
    let selected_destination = parse_destination_hex(selected_hash)?;
    let guard = peer_crypto.lock().ok()?;
    if guard.contains_key(selected_hash) {
        return Some(selected_hash.to_string());
    }
    for (destination_hash, crypto) in guard.iter() {
        if crypto.identity.address_hash.as_slice() == selected_destination {
            return Some(destination_hash.clone());
        }
    }
    None
}

fn parse_destination_hex(input: &str) -> Option<[u8; 16]> {
    let bytes = hex::decode(input).ok()?;
    let mut out = [0u8; 16];
    match bytes.len() {
        16 => {
            out.copy_from_slice(&bytes);
            Some(out)
        }
        32 => {
            out.copy_from_slice(&bytes[..16]);
            Some(out)
        }
        _ => None,
    }
}

fn parse_destination_hex_required(input: &str) -> Result<[u8; 16], std::io::Error> {
    parse_destination_hex(input).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid destination hash '{input}' (expected 16-byte or 32-byte hex)"),
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

fn annotate_inbound_transport_metadata(
    record: &mut MessageRecord,
    event: &reticulum::transport::ReceivedData,
) {
    let mut transport = serde_json::Map::new();
    transport.insert("ratchet_used".to_string(), Value::Bool(event.ratchet_used));

    let mut root = match record.fields.take() {
        Some(Value::Object(existing)) => existing,
        Some(other) => {
            let mut root = serde_json::Map::new();
            root.insert("_fields_raw".to_string(), other);
            root
        }
        None => serde_json::Map::new(),
    };
    root.insert("_transport".to_string(), Value::Object(transport));
    record.fields = Some(Value::Object(root));
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
            fields: message.fields.as_ref().and_then(json_fields_with_raw_preserved),
            receipt_status: None,
        });
    }

    if !relaxed_decode_enabled() {
        return None;
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
        fields: decoded.fields.as_ref().and_then(json_fields_with_raw_preserved),
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

fn relaxed_decode_enabled() -> bool {
    static RELAXED_DECODE: OnceLock<bool> = OnceLock::new();
    *RELAXED_DECODE.get_or_init(|| {
        parse_env_bool("LXMF_RUNTIME_RELAXED_DECODE")
            .or_else(|| parse_env_bool("LXMF_ALLOW_RELAXED_DECODE"))
            .unwrap_or(false)
    })
}

fn parse_env_bool(key: &str) -> Option<bool> {
    let raw = std::env::var(key).ok()?;
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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
        message.fields = Some(wire_fields_from_json(&fields)?);
    }
    message.to_wire(Some(signer))
}

fn wire_fields_from_json(value: &Value) -> Result<rmpv::Value, LxmfError> {
    if let Some(raw) = decode_transport_fields_json(value)? {
        return Ok(raw);
    }
    json_to_rmpv(value)
}

fn json_to_rmpv(value: &Value) -> Result<rmpv::Value, LxmfError> {
    json_to_rmpv_lossless(value)
}

fn json_to_rmpv_lossless(value: &Value) -> Result<rmpv::Value, LxmfError> {
    match value {
        Value::Null => Ok(rmpv::Value::Nil),
        Value::Bool(v) => Ok(rmpv::Value::Boolean(*v)),
        Value::Number(number) => {
            if let Some(raw) = number.as_i64() {
                return Ok(rmpv::Value::Integer(raw.into()));
            }
            if let Some(raw) = number.as_u64() {
                return Ok(rmpv::Value::Integer(raw.into()));
            }
            number
                .as_f64()
                .map(rmpv::Value::F64)
                .ok_or_else(|| LxmfError::Encode("invalid json number".to_string()))
        }
        Value::String(text) => Ok(rmpv::Value::String(text.as_str().into())),
        Value::Array(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(json_to_rmpv_lossless(value)?);
            }
            Ok(rmpv::Value::Array(out))
        }
        Value::Object(map) => {
            let mut entries = Vec::with_capacity(map.len());
            for (key, value) in map {
                let key = json_object_key_to_rmpv(key);
                entries.push((key, json_to_rmpv_lossless(value)?));
            }
            Ok(rmpv::Value::Map(entries))
        }
    }
}

fn json_object_key_to_rmpv(key: &str) -> rmpv::Value {
    if let Ok(value) = key.parse::<i64>() {
        return rmpv::Value::Integer(value.into());
    }
    if let Ok(value) = key.parse::<u64>() {
        return rmpv::Value::Integer(value.into());
    }
    rmpv::Value::String(key.into())
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
                if key_str == "2" {
                    if let rmpv::Value::Binary(bytes) = value {
                        if let Some(decoded) = decode_sideband_location_telemetry(bytes) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                }
                if key_str == "3" {
                    if let rmpv::Value::Binary(bytes) = value {
                        if let Some(decoded) = decode_telemetry_stream(bytes) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                }
                if key_str == "112" {
                    if let rmpv::Value::Binary(bytes) = value {
                        if let Some(decoded) = decode_columba_meta(bytes) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                }
                object.insert(key_str, rmpv_to_json(value)?);
            }
            enrich_app_extension_fields(&mut object);
            Some(Value::Object(object))
        }
        _ => None,
    }
}

fn json_fields_with_raw_preserved(value: &rmpv::Value) -> Option<Value> {
    let mut converted = rmpv_to_json(value)?;
    if let Value::Object(object) = &mut converted {
        if let Ok(raw) = rmp_serde::to_vec(value) {
            object.insert(
                "_lxmf_fields_msgpack_b64".to_string(),
                Value::String(BASE64_STANDARD.encode(raw)),
            );
        }
    }
    Some(converted)
}

fn sanitize_outbound_wire_fields(fields: Option<&Value>) -> Option<Value> {
    let Some(Value::Object(fields)) = fields else {
        return fields.cloned();
    };

    let mut out = fields.clone();
    out.remove(OUTBOUND_DELIVERY_OPTIONS_FIELD);

    for key in ["_lxmf", "lxmf"] {
        let Some(Value::Object(lxmf_fields)) = out.get_mut(key) else {
            continue;
        };
        for reserved in [
            "method",
            "stamp_cost",
            "include_ticket",
            "try_propagation_on_fail",
            "source_private_key",
        ] {
            lxmf_fields.remove(reserved);
        }
        if lxmf_fields.is_empty() {
            out.remove(key);
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}

fn decode_sideband_location_telemetry(packed: &[u8]) -> Option<Value> {
    let mut cursor = std::io::Cursor::new(packed);
    let decoded = rmpv::decode::read_value(&mut cursor).ok()?;
    let rmpv::Value::Map(map) = decoded else {
        return None;
    };
    let location = map
        .iter()
        .find(|(key, _)| key.as_i64() == Some(0x02) || key.as_u64() == Some(0x02))
        .map(|(_, value)| value)?;
    let rmpv::Value::Array(items) = location else {
        return None;
    };
    if items.len() < 7 {
        return None;
    }

    let lat = decode_i32_be(items.first()?)? as f64 / 1e6;
    let lon = decode_i32_be(items.get(1)?)? as f64 / 1e6;
    let alt = decode_i32_be(items.get(2)?)? as f64 / 1e2;
    let speed = decode_u32_be(items.get(3)?)? as f64 / 1e2;
    let bearing = decode_i32_be(items.get(4)?)? as f64 / 1e2;
    let accuracy = decode_u16_be(items.get(5)?)? as f64 / 1e2;
    let updated = items.get(6).and_then(|value| {
        value.as_i64().or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
    });

    let mut out = serde_json::Map::new();
    out.insert("lat".to_string(), Value::from(lat));
    out.insert("lon".to_string(), Value::from(lon));
    out.insert("alt".to_string(), Value::from(alt));
    out.insert("speed".to_string(), Value::from(speed));
    out.insert("bearing".to_string(), Value::from(bearing));
    out.insert("accuracy".to_string(), Value::from(accuracy));
    if let Some(updated) = updated {
        out.insert("updated".to_string(), Value::from(updated));
    }
    Some(Value::Object(out))
}

fn decode_telemetry_stream(packed: &[u8]) -> Option<Value> {
    let mut cursor = std::io::Cursor::new(packed);
    let decoded = rmpv::decode::read_value(&mut cursor).ok()?;
    rmpv_to_json(&decoded)
}

fn decode_columba_meta(packed: &[u8]) -> Option<Value> {
    if let Ok(text) = std::str::from_utf8(packed) {
        if let Ok(json) = serde_json::from_str::<Value>(text) {
            return Some(json);
        }
        return Some(Value::String(text.to_string()));
    }
    let mut cursor = std::io::Cursor::new(packed);
    let decoded = rmpv::decode::read_value(&mut cursor).ok()?;
    rmpv_to_json(&decoded)
}

fn enrich_app_extension_fields(object: &mut serde_json::Map<String, Value>) {
    let Some(app_extensions) = object.get("16").and_then(Value::as_object).cloned() else {
        return;
    };

    if let Some(reaction_to) = app_extensions.get("reaction_to").and_then(Value::as_str) {
        object.insert("is_reaction".to_string(), Value::Bool(true));
        object.insert("reaction_to".to_string(), Value::String(reaction_to.to_string()));
        if let Some(emoji) = app_extensions.get("emoji").and_then(Value::as_str) {
            object.insert("reaction_emoji".to_string(), Value::String(emoji.to_string()));
        }
        if let Some(sender) = app_extensions.get("sender").and_then(Value::as_str) {
            object.insert("reaction_sender".to_string(), Value::String(sender.to_string()));
        }
    }

    if let Some(reply_to) = app_extensions.get("reply_to").and_then(Value::as_str) {
        object.insert("reply_to".to_string(), Value::String(reply_to.to_string()));
    }
}

fn decode_binary_bytes(value: &rmpv::Value) -> Option<&[u8]> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.as_slice()),
        _ => None,
    }
}

fn decode_i32_be(value: &rmpv::Value) -> Option<i32> {
    let bytes = decode_binary_bytes(value)?;
    if bytes.len() != 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(bytes);
    Some(i32::from_be_bytes(raw))
}

fn decode_u32_be(value: &rmpv::Value) -> Option<u32> {
    let bytes = decode_binary_bytes(value)?;
    if bytes.len() != 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(bytes);
    Some(u32::from_be_bytes(raw))
}

fn decode_u16_be(value: &rmpv::Value) -> Option<u16> {
    let bytes = decode_binary_bytes(value)?;
    if bytes.len() != 2 {
        return None;
    }
    let mut raw = [0u8; 2];
    raw.copy_from_slice(bytes);
    Some(u16::from_be_bytes(raw))
}

fn encode_delivery_display_name_app_data(display_name: &str) -> Option<Vec<u8>> {
    let peer_data = rmpv::Value::Array(vec![
        rmpv::Value::Binary(display_name.as_bytes().to_vec()),
        rmpv::Value::Nil,
    ]);
    rmp_serde::to_vec(&peer_data).ok()
}

fn encode_propagation_node_app_data(display_name: Option<&str>) -> Option<Vec<u8>> {
    let mut metadata = Vec::new();
    if let Some(name) = display_name {
        metadata.push((
            rmpv::Value::Integer((crate::constants::PN_META_NAME as i64).into()),
            rmpv::Value::Binary(name.as_bytes().to_vec()),
        ));
    }

    let announce_data = rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::Integer((now_epoch_secs() as i64).into()),
        rmpv::Value::Boolean(true),
        rmpv::Value::Integer((crate::constants::PROPAGATION_LIMIT as i64).into()),
        rmpv::Value::Integer((crate::constants::SYNC_LIMIT as i64).into()),
        rmpv::Value::Array(vec![
            rmpv::Value::Integer((crate::constants::PROPAGATION_COST as i64).into()),
            rmpv::Value::Integer((crate::constants::PROPAGATION_COST_FLEX as i64).into()),
            rmpv::Value::Integer((crate::constants::PEERING_COST as i64).into()),
        ]),
        rmpv::Value::Map(metadata),
    ]);
    rmp_serde::to_vec(&announce_data).ok()
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

fn lxmf_aspect_from_name_hash(name_hash: &[u8]) -> Option<String> {
    let delivery = DestinationName::new("lxmf", "delivery");
    if name_hash == delivery.as_name_hash_slice() {
        return Some("lxmf.delivery".to_string());
    }

    let propagation = DestinationName::new("lxmf", "propagation");
    if name_hash == propagation.as_name_hash_slice() {
        return Some("lxmf.propagation".to_string());
    }

    let rmsp_maps = DestinationName::new("rmsp", "maps");
    if name_hash == rmsp_maps.as_name_hash_slice() {
        return Some("rmsp.maps".to_string());
    }

    None
}

fn update_peer_announce_meta(
    peer_announce_meta: &Arc<Mutex<HashMap<String, PeerAnnounceMeta>>>,
    peer: &str,
    app_data: &[u8],
) {
    let app_data_hex = if app_data.is_empty() { None } else { Some(hex::encode(app_data)) };

    let mut guard = peer_announce_meta.lock().expect("peer metadata map");
    guard.insert(peer.to_string(), PeerAnnounceMeta { app_data_hex });
}

fn annotate_peer_records_with_announce_metadata(
    result: &mut Value,
    metadata: &HashMap<String, PeerAnnounceMeta>,
) {
    if metadata.is_empty() {
        return;
    }

    if let Some(object) = result.as_object_mut() {
        if let Some(Value::Array(peers)) = object.get_mut("peers") {
            annotate_peer_array(peers, metadata);
        }
        return;
    }

    if let Value::Array(peers) = result {
        annotate_peer_array(peers, metadata);
    }
}

fn annotate_peer_array(peers: &mut [Value], metadata: &HashMap<String, PeerAnnounceMeta>) {
    for peer in peers {
        let Some(record) = peer.as_object_mut() else {
            continue;
        };
        let Some(peer_hash) = record.get("peer").and_then(Value::as_str) else {
            continue;
        };
        let Some(meta) = metadata.get(peer_hash) else {
            continue;
        };
        if let Some(app_data_hex) = meta.app_data_hex.as_ref() {
            record.insert("app_data_hex".to_string(), Value::String(app_data_hex.clone()));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPeerIdentity {
    destination: String,
    identity_hex: String,
}

fn load_peer_identity_cache(path: &Path) -> Result<HashMap<String, PeerCrypto>, LxmfError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(path).map_err(|err| LxmfError::Io(err.to_string()))?;
    let entries: Vec<PersistedPeerIdentity> =
        serde_json::from_str(&raw).map_err(|err| LxmfError::Decode(err.to_string()))?;
    let mut out = HashMap::new();
    for entry in entries {
        let Some(destination) = normalize_hash_hex_16(&entry.destination) else {
            continue;
        };
        let Ok(identity) = Identity::new_from_hex_string(entry.identity_hex.trim()) else {
            continue;
        };
        out.insert(destination, PeerCrypto { identity });
    }
    Ok(out)
}

fn persist_peer_identity_cache(peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>, path: &Path) {
    let snapshot = peer_crypto
        .lock()
        .map(|guard| {
            let mut entries = guard
                .iter()
                .map(|(destination, crypto)| PersistedPeerIdentity {
                    destination: destination.clone(),
                    identity_hex: crypto.identity.to_hex_string(),
                })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.destination.cmp(&b.destination));
            entries
        })
        .unwrap_or_default();

    let encoded = match serde_json::to_string_pretty(&snapshot) {
        Ok(encoded) => encoded,
        Err(_) => return,
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, encoded).is_ok() {
        let _ = fs::rename(&tmp, path);
    } else {
        let _ = fs::remove_file(&tmp);
    }
}

fn apply_runtime_identity_restore(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    cache_path: &Path,
    method: &str,
    params: Option<&Value>,
) {
    let Some(params) = params.and_then(Value::as_object) else {
        return;
    };

    match method {
        "store_peer_identity" => {
            let identity_hash = params.get("identity_hash").and_then(Value::as_str);
            let public_key = params.get("public_key").and_then(Value::as_str);
            if let (Some(identity_hash), Some(public_key)) = (identity_hash, public_key) {
                register_peer_identity(peer_crypto, identity_hash, public_key);
            }
        }
        "restore_all_peer_identities" | "bulk_restore_peer_identities" => {
            if let Some(peers) = params.get("peers").and_then(Value::as_array) {
                for peer in peers {
                    let Some(peer) = peer.as_object() else {
                        continue;
                    };
                    let identity_hash = peer.get("identity_hash").and_then(Value::as_str);
                    let public_key = peer.get("public_key").and_then(Value::as_str);
                    if let (Some(identity_hash), Some(public_key)) = (identity_hash, public_key) {
                        register_peer_identity(peer_crypto, identity_hash, public_key);
                    }
                }
            }
        }
        "bulk_restore_announce_identities" => {
            if let Some(announces) = params.get("announces").and_then(Value::as_array) {
                for announce in announces {
                    let Some(announce) = announce.as_object() else {
                        continue;
                    };
                    let destination = announce.get("destination_hash").and_then(Value::as_str);
                    let public_key = announce.get("public_key").and_then(Value::as_str);
                    if let (Some(destination), Some(public_key)) = (destination, public_key) {
                        register_destination_identity(peer_crypto, destination, public_key);
                    }
                }
            }
        }
        _ => {}
    }

    persist_peer_identity_cache(peer_crypto, cache_path);
}

fn register_peer_identity(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    identity_hash_hex: &str,
    public_key_material: &str,
) {
    let Some(identity_hash) = normalize_hash_hex_16(identity_hash_hex) else {
        return;
    };
    let Some(identity) = identity_from_public_key_material(public_key_material) else {
        return;
    };

    let destination =
        SingleOutputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = hex::encode(destination.desc.address_hash.as_slice());

    if identity_hash != hex::encode(identity.address_hash.as_slice()) {
        // Public key material is source of truth for identity derivation.
    }

    if let Ok(mut guard) = peer_crypto.lock() {
        guard.insert(destination_hash, PeerCrypto { identity });
    }
}

fn register_destination_identity(
    peer_crypto: &Arc<Mutex<HashMap<String, PeerCrypto>>>,
    destination_hash: &str,
    public_key_material: &str,
) {
    let Some(destination_hash) = normalize_hash_hex_16(destination_hash) else {
        return;
    };
    let Some(identity) = identity_from_public_key_material(public_key_material) else {
        return;
    };
    if let Ok(mut guard) = peer_crypto.lock() {
        guard.insert(destination_hash, PeerCrypto { identity });
    }
}

fn identity_from_public_key_material(public_key_material: &str) -> Option<Identity> {
    let bytes = decode_key_material_bytes(public_key_material)?;
    if bytes.len() != 64 {
        return None;
    }
    Some(Identity::new_from_slices(&bytes[..32], &bytes[32..64]))
}

fn decode_key_material_bytes(value: &str) -> Option<Vec<u8>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() % 2 == 0 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return hex::decode(trimmed).ok();
    }
    BASE64_STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .ok()
}

fn normalize_hash_hex_16(value: &str) -> Option<String> {
    let bytes = hex::decode(value.trim()).ok()?;
    let mut normalized = [0u8; 16];
    match bytes.len() {
        16 => normalized.copy_from_slice(&bytes),
        32 => normalized.copy_from_slice(&bytes[..16]),
        _ => return None,
    }
    Some(hex::encode(normalized))
}

fn annotate_response_meta(result: &mut Value, profile: &str, rpc_endpoint: &str) {
    let Some(root) = result.as_object_mut() else {
        return;
    };
    if !root.get("meta").map(Value::is_object).unwrap_or(false) {
        root.insert("meta".to_string(), serde_json::json!({}));
    }
    let Some(meta) = root.get_mut("meta").and_then(Value::as_object_mut) else {
        return;
    };

    if meta.get("contract_version").map(Value::is_null).unwrap_or(true) {
        meta.insert("contract_version".to_string(), Value::String("v2".to_string()));
    }
    if meta.get("profile").map(Value::is_null).unwrap_or(true) {
        meta.insert("profile".to_string(), Value::String(profile.to_string()));
    }
    if meta.get("rpc_endpoint").map(Value::is_null).unwrap_or(true) {
        meta.insert("rpc_endpoint".to_string(), Value::String(rpc_endpoint.to_string()));
    }
}

fn build_send_params_with_source(
    request: SendMessageRequest,
    source: String,
) -> Result<PreparedSendMessage, LxmfError> {
    let destination = clean_non_empty(Some(request.destination))
        .ok_or_else(|| LxmfError::Io("destination is required".to_string()))?;
    let id = clean_non_empty(request.id).unwrap_or_else(generate_message_id);

    let mut params = json!({
        "id": id,
        "source": source,
        "destination": destination,
        "title": request.title,
        "content": request.content,
    });

    if let Some(fields) = request.fields {
        params["fields"] = fields;
    }
    if let Some(method) = clean_non_empty(request.method) {
        params["method"] = Value::String(method);
    }
    if let Some(stamp_cost) = request.stamp_cost {
        params["stamp_cost"] = Value::from(stamp_cost);
    }
    if request.include_ticket {
        params["include_ticket"] = Value::Bool(true);
    }
    if request.try_propagation_on_fail {
        params["try_propagation_on_fail"] = Value::Bool(true);
    }
    if let Some(source_private_key) = clean_non_empty(request.source_private_key) {
        params["source_private_key"] = Value::String(source_private_key);
    }

    Ok(PreparedSendMessage { id, source, destination, params })
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

fn source_hash_from_private_key_hex(private_key_hex: &str) -> Result<String, LxmfError> {
    let key_bytes = hex::decode(private_key_hex.trim())
        .map_err(|_| LxmfError::Io("source_private_key must be hex-encoded".to_string()))?;
    let identity = PrivateIdentity::from_private_key_bytes(&key_bytes)
        .map_err(|_| LxmfError::Io("source_private_key is invalid".to_string()))?;
    Ok(hex::encode(identity.address_hash().as_slice()))
}

fn generate_message_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("lxmf-{now}")
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
    use super::{
        annotate_peer_records_with_announce_metadata, annotate_response_meta,
        build_propagation_envelope, build_send_params_with_source, build_wire_message,
        decode_inbound_payload, format_relay_request_status, normalize_relay_destination_hash,
        parse_alternative_relay_request_status, propagation_relay_candidates, rmpv_to_json,
        sanitize_outbound_wire_fields, PeerAnnounceMeta, PeerCrypto,
    };
    use crate::constants::FIELD_COMMANDS;
    use crate::message::Message;
    use crate::payload_fields::{CommandEntry, WireFields};
    use crate::propagation::unpack_envelope;
    use crate::runtime::SendMessageRequest;
    use reticulum::identity::PrivateIdentity;
    use serde_json::{json, Value};
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn build_wire_message_prefers_transport_msgpack_fields() {
        let mut fields = WireFields::new();
        fields.set_commands(vec![CommandEntry::from_text(0x01, "ping")]);
        let json_fields = fields.to_transport_json().expect("transport fields");

        let signer = PrivateIdentity::new_from_name("wire-fields-test");
        let source = [0x10; 16];
        let destination = [0x20; 16];
        let wire =
            build_wire_message(source, destination, "title", "content", Some(json_fields), &signer)
                .expect("wire");

        let decoded = Message::from_wire(&wire).expect("decode");
        let Some(rmpv::Value::Map(entries)) = decoded.fields else {
            panic!("fields should decode to map")
        };
        let commands = entries
            .iter()
            .find_map(|(key, value)| (key.as_i64() == Some(FIELD_COMMANDS as i64)).then_some(value))
            .expect("commands field");
        let rmpv::Value::Array(commands_list) = commands else {
            panic!("commands should be an array")
        };
        assert_eq!(commands_list.len(), 1);
    }

    #[test]
    fn build_send_params_includes_expected_rpc_keys() {
        let request = SendMessageRequest {
            id: Some("msg-123".to_string()),
            source: Some("ignored".to_string()),
            source_private_key: Some(
                "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff".to_string(),
            ),
            destination: "ffeeddccbbaa99887766554433221100".to_string(),
            title: "subject".to_string(),
            content: "body".to_string(),
            fields: Some(serde_json::json!({ "k": "v" })),
            method: Some("direct".to_string()),
            stamp_cost: Some(7),
            include_ticket: true,
            try_propagation_on_fail: true,
        };

        let prepared =
            build_send_params_with_source(request, "00112233445566778899aabbccddeeff".to_string())
                .expect("prepared");
        assert_eq!(prepared.id, "msg-123");
        assert_eq!(prepared.source, "00112233445566778899aabbccddeeff");
        assert_eq!(prepared.destination, "ffeeddccbbaa99887766554433221100");
        assert_eq!(prepared.params["method"], Value::String("direct".to_string()));
        assert_eq!(prepared.params["stamp_cost"], Value::from(7));
        assert_eq!(prepared.params["include_ticket"], Value::Bool(true));
        assert_eq!(prepared.params["try_propagation_on_fail"], Value::Bool(true));
        assert_eq!(
            prepared.params["source_private_key"],
            Value::String(
                "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff".to_string()
            )
        );
        assert_eq!(prepared.params["fields"]["k"], Value::String("v".to_string()));
    }

    #[test]
    fn sanitize_outbound_wire_fields_removes_transport_controls() {
        let fields = json!({
            "__delivery_options": {
                "method": "propagated",
                "stamp_cost": 128,
                "include_ticket": true
            },
            "_lxmf": {
                "method": "direct",
                "scope": "chat",
                "app": "weft",
            },
            "lxmf": {
                "try_propagation_on_fail": true,
                "app": "bridge",
            },
            "attachments": [],
        });
        let sanitized = sanitize_outbound_wire_fields(Some(&fields)).expect("sanitized");
        assert!(sanitized.get("__delivery_options").is_none());

        let Some(sanitized_lxmf) = sanitized.get("_lxmf").and_then(Value::as_object) else {
            panic!("_lxmf preserved")
        };
        assert!(sanitized_lxmf.get("method").is_none());
        assert_eq!(sanitized_lxmf.get("scope"), Some(&Value::String("chat".to_string())));
        assert_eq!(sanitized_lxmf.get("app"), Some(&Value::String("weft".to_string())));

        let Some(sanitized_alt_lxmf) = sanitized.get("lxmf").and_then(Value::as_object) else {
            panic!("lxmf preserved")
        };
        assert!(sanitized_alt_lxmf.get("try_propagation_on_fail").is_none());
        assert_eq!(sanitized_alt_lxmf.get("app"), Some(&Value::String("bridge".to_string())));
        assert_eq!(sanitized.get("attachments"), Some(&Value::Array(vec![])));
    }

    #[test]
    fn build_send_params_rejects_empty_destination() {
        let request = SendMessageRequest {
            destination: "   ".to_string(),
            content: "body".to_string(),
            ..SendMessageRequest::default()
        };
        let err = build_send_params_with_source(request, "source".to_string()).expect_err("err");
        assert!(err.to_string().contains("destination is required"));
    }

    #[test]
    fn annotate_list_peers_result_with_app_data_hex() {
        let mut result = serde_json::json!({
            "peers": [
                { "peer": "aa11", "last_seen": 1 },
                { "peer": "bb22", "last_seen": 2 }
            ]
        });
        let mut metadata = HashMap::new();
        metadata.insert(
            "aa11".to_string(),
            PeerAnnounceMeta { app_data_hex: Some("cafe".to_string()) },
        );

        annotate_peer_records_with_announce_metadata(&mut result, &metadata);
        assert_eq!(result["peers"][0]["app_data_hex"], Value::String("cafe".to_string()));
        assert_eq!(result["peers"][1]["app_data_hex"], Value::Null);
    }

    #[test]
    fn annotate_response_meta_populates_profile_and_rpc() {
        let mut result = serde_json::json!({
            "nodes": [],
            "meta": {
                "contract_version": "v2",
                "profile": null,
                "rpc_endpoint": null
            }
        });

        annotate_response_meta(&mut result, "weft2", "127.0.0.1:4243");
        assert_eq!(result["meta"]["contract_version"], "v2");
        assert_eq!(result["meta"]["profile"], "weft2");
        assert_eq!(result["meta"]["rpc_endpoint"], "127.0.0.1:4243");
    }

    #[test]
    fn annotate_response_meta_creates_meta_when_missing() {
        let mut result = serde_json::json!({
            "messages": []
        });

        annotate_response_meta(&mut result, "weft2", "127.0.0.1:4243");
        assert_eq!(result["meta"]["contract_version"], "v2");
        assert_eq!(result["meta"]["profile"], "weft2");
        assert_eq!(result["meta"]["rpc_endpoint"], "127.0.0.1:4243");
    }

    #[test]
    fn annotate_response_meta_preserves_existing_non_null_values() {
        let mut result = serde_json::json!({
            "messages": [],
            "meta": {
                "contract_version": "v9",
                "profile": "custom",
                "rpc_endpoint": "192.168.1.10:9999"
            }
        });

        annotate_response_meta(&mut result, "weft2", "127.0.0.1:4243");
        assert_eq!(result["meta"]["contract_version"], "v9");
        assert_eq!(result["meta"]["profile"], "custom");
        assert_eq!(result["meta"]["rpc_endpoint"], "192.168.1.10:9999");
    }

    #[test]
    fn normalize_relay_destination_hash_preserves_destination_hash_input() {
        let destination_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let signer = PrivateIdentity::new_from_name("relay-preserve");
        let identity = *signer.as_identity();
        let mut peer_map = HashMap::new();
        peer_map.insert(destination_hash.clone(), PeerCrypto { identity });
        let peer_crypto = Arc::new(Mutex::new(peer_map));

        let resolved = normalize_relay_destination_hash(&peer_crypto, &destination_hash)
            .expect("should preserve known destination hash");
        assert_eq!(resolved, destination_hash);
    }

    #[test]
    fn normalize_relay_destination_hash_maps_identity_hash_to_destination_hash() {
        let destination_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        let signer = PrivateIdentity::new_from_name("relay-normalize");
        let identity = *signer.as_identity();
        let identity_hash = hex::encode(identity.address_hash.as_slice());
        let mut peer_map = HashMap::new();
        peer_map.insert(destination_hash.clone(), PeerCrypto { identity });
        let peer_crypto = Arc::new(Mutex::new(peer_map));

        let resolved = normalize_relay_destination_hash(&peer_crypto, &identity_hash)
            .expect("should map known identity hash to destination hash");
        assert_eq!(resolved, destination_hash);
    }

    #[test]
    fn propagation_relay_candidates_prefer_selected_then_known_nodes() {
        let selected = Arc::new(Mutex::new(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string())));
        let known_nodes = Arc::new(Mutex::new(HashSet::from([
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "cccccccccccccccccccccccccccccccc".to_string(),
        ])));

        let candidates = propagation_relay_candidates(&selected, &known_nodes);
        assert_eq!(candidates[0], "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(candidates.contains(&"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()));
        assert!(candidates.contains(&"cccccccccccccccccccccccccccccccc".to_string()));
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn relay_request_status_roundtrips_exclusions() {
        let status = format_relay_request_status(&[
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        ]);
        let excludes =
            parse_alternative_relay_request_status(status.as_str()).expect("relay request status");
        assert_eq!(
            excludes,
            vec![
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()
            ]
        );
    }

    #[test]
    fn build_propagation_envelope_wraps_wire_payload() {
        let signer = PrivateIdentity::new_from_name("propagation-envelope-signer");
        let recipient = PrivateIdentity::new_from_name("propagation-envelope-recipient");
        let mut source = [0u8; 16];
        source.copy_from_slice(signer.address_hash().as_slice());
        let destination = [0x44; 16];
        let wire = build_wire_message(source, destination, "", "hello", None, &signer)
            .expect("wire payload");

        let envelope = build_propagation_envelope(&wire, recipient.as_identity())
            .expect("propagation envelope");
        let unpacked = unpack_envelope(&envelope).expect("decode propagation envelope");
        assert_eq!(unpacked.messages.len(), 1);
        assert_eq!(&unpacked.messages[0][..16], destination.as_slice());
        assert_ne!(unpacked.messages[0], wire);
    }

    #[test]
    fn rmpv_to_json_decodes_sideband_packed_location_sensor() {
        let packed = rmp_serde::to_vec(&rmpv::Value::Map(vec![
            (rmpv::Value::Integer(1_i64.into()), rmpv::Value::Integer(1_770_855_315_i64.into())),
            (
                rmpv::Value::Integer(2_i64.into()),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary((48_856_600_i32).to_be_bytes().to_vec()),
                    rmpv::Value::Binary((2_352_200_i32).to_be_bytes().to_vec()),
                    rmpv::Value::Binary((3550_i32).to_be_bytes().to_vec()),
                    rmpv::Value::Binary((420_u32).to_be_bytes().to_vec()),
                    rmpv::Value::Binary((18_000_i32).to_be_bytes().to_vec()),
                    rmpv::Value::Binary((340_u16).to_be_bytes().to_vec()),
                    rmpv::Value::Integer(1_770_855_315_i64.into()),
                ]),
            ),
        ]))
        .expect("pack telemetry");

        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::Integer(2_i64.into()),
            rmpv::Value::Binary(packed),
        )]);
        let decoded = rmpv_to_json(&fields).expect("decoded");

        assert_eq!(decoded["2"]["lat"], serde_json::json!(48.8566));
        assert_eq!(decoded["2"]["lon"], serde_json::json!(2.3522));
        assert_eq!(decoded["2"]["accuracy"], serde_json::json!(3.4));
        assert_eq!(decoded["2"]["updated"], serde_json::json!(1_770_855_315_i64));
    }
}
