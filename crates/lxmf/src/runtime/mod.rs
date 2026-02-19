mod announce_helpers;
mod announce_rate_limit;
mod bootstrap;
mod identity_io;
mod inbound_helpers;
mod peer_cache;
mod propagation_link;
mod receipt_flow;
mod receipt_helpers;
mod relay_helpers;
mod request_handlers;
mod send_helpers;
mod send_pipeline;
mod startup_workers;
mod support;
mod wire_codec;

use crate::cli::daemon::DaemonStatus;
use crate::cli::profile::{
    load_profile_settings, load_reticulum_config, profile_paths, resolve_identity_path,
    resolve_runtime_profile_name, InterfaceEntry, ProfilePaths, ProfileSettings,
};
use crate::helpers::normalize_display_name;
#[cfg(test)]
use crate::inbound_decode::InboundPayloadMode;
use crate::payload_fields::{CommandEntry, WireFields};
use crate::LxmfError;
use announce_helpers::{
    annotate_peer_records_with_announce_metadata, encode_delivery_display_name_app_data,
    encode_propagation_node_app_data,
};
use announce_rate_limit::trigger_rate_limited_announce;
use identity_io::{drop_empty_identity_stub, load_or_create_identity};
use inbound_helpers::build_propagation_envelope;
#[cfg(test)]
use inbound_helpers::decode_inbound_payload;
use peer_cache::{
    apply_runtime_identity_restore, load_peer_identity_cache, persist_peer_identity_cache,
};
use receipt_flow::{handle_receipt_event, resolve_link_destination, ReceiptBridge, ReceiptEvent};
use receipt_helpers::{
    format_relay_request_status, is_message_marked_delivered,
    parse_alternative_relay_request_status, prune_receipt_mappings_for_message,
    track_outbound_resource_mapping, track_receipt_mapping,
};
use relay_helpers::{
    normalize_relay_destination_hash, parse_destination_hex, parse_destination_hex_required,
    propagation_relay_candidates, short_hash_prefix, wait_for_external_relay_selection,
};
use request_handlers::handle_runtime_request;
use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::identity::{Identity, PrivateIdentity};
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::rpc::{
    AnnounceBridge, InterfaceRecord, OutboundBridge, RpcDaemon, RpcEvent, RpcRequest,
};
use reticulum::storage::messages::{MessageRecord, MessagesStore};
use reticulum::transport::{Transport, TransportConfig};
use send_helpers::{
    can_send_opportunistic, opportunistic_payload, parse_delivery_method, send_outcome_is_sent,
    send_outcome_status, DeliveryMethod,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use startup_workers::{
    spawn_receipt_worker, spawn_startup_announce_burst, spawn_transport_workers,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use support::{
    clean_non_empty, extract_identity_hash, generate_message_id, interface_to_rpc,
    parse_bind_host_port, source_hash_from_private_key_hex,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;
use tokio::task::LocalSet;
use wire_codec::{build_wire_message, sanitize_outbound_wire_fields};
#[cfg(test)]
use wire_codec::{json_to_rmpv, rmpv_to_json};

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

#[cfg(reticulum_api_v2)]
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

#[cfg(not(reticulum_api_v2))]
fn merge_outbound_delivery_options(record: &MessageRecord) -> OutboundDeliveryOptionsCompat {
    extract_outbound_delivery_options(record)
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

impl WorkerState {
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
    #[cfg(reticulum_api_v2)]
    fn deliver(
        &self,
        record: &MessageRecord,
        options: &reticulum::rpc::OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        self.deliver_with_options(record, merge_outbound_delivery_options(options, record))
    }

    #[cfg(not(reticulum_api_v2))]
    fn deliver(&self, record: &MessageRecord) -> Result<(), std::io::Error> {
        self.deliver_with_options(record, merge_outbound_delivery_options(record))
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

fn now_epoch_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
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

#[cfg(test)]
mod tests {
    use super::{
        annotate_peer_records_with_announce_metadata, annotate_response_meta,
        build_propagation_envelope, build_send_params_with_source, build_wire_message,
        can_send_opportunistic, decode_inbound_payload, format_relay_request_status, json_to_rmpv,
        normalize_relay_destination_hash, parse_alternative_relay_request_status,
        propagation_relay_candidates, rmpv_to_json, sanitize_outbound_wire_fields,
        InboundPayloadMode, PeerAnnounceMeta, PeerCrypto,
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

        let record = decode_inbound_payload(destination, &wire, InboundPayloadMode::FullWire)
            .expect("decoded record");
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
    fn sanitize_outbound_wire_fields_preserves_canonical_attachments() {
        let fields = json!({
            "attachments": [
                {
                    "name": "sideband_note.txt",
                    "data": [110, 111, 116, 101],
                    "media_type": "text/plain",
                },
                {
                    "name": "legacy.json",
                    "data": [123, 125]
                }
            ],
        });
        let sanitized = sanitize_outbound_wire_fields(Some(&fields)).expect("sanitized");
        assert_eq!(sanitized.get("attachments"), fields.get("attachments"));
    }

    #[test]
    fn build_wire_message_rejects_ambiguous_attachment_text_data() {
        let signer = PrivateIdentity::new_from_name("runtime-ambiguous-attachment");
        let source = [0x10; 16];
        let destination = [0x20; 16];
        let fields = json!({
            "attachments": [
                {
                    "name": "ambiguous.bin",
                    "data": "deadbeef",
                },
            ],
        });
        let err =
            build_wire_message(source, destination, "title", "content", Some(fields), &signer)
                .expect_err("ambiguous attachment text must fail");
        assert!(err.to_string().contains("attachment text data must use explicit"));
    }

    #[test]
    fn build_wire_message_accepts_prefixed_attachment_data() {
        let signer = PrivateIdentity::new_from_name("runtime-prefixed-attachment");
        let source = [0x30; 16];
        let destination = [0x40; 16];
        let fields = json!({
            "attachments": [
                {
                    "name": "hex.bin",
                    "data": "hex:0a0b0c",
                },
                {
                    "name": "b64.bin",
                    "data": "base64:AQID",
                },
            ]
        });
        let wire =
            build_wire_message(source, destination, "title", "content", Some(fields), &signer)
                .expect("wire");
        let decoded = Message::from_wire(&wire).expect("decode");
        let parsed = decoded.fields.as_ref().and_then(rmpv_to_json).expect("fields");
        assert_eq!(
            parsed.get("5"),
            Some(&json!([["hex.bin", [10, 11, 12]], ["b64.bin", [1, 2, 3]]]))
        );
    }

    #[test]
    fn build_wire_message_rejects_invalid_attachment_entries() {
        let signer = PrivateIdentity::new_from_name("runtime-invalid-attachment-entry");
        let source = [0x50; 16];
        let destination = [0x60; 16];
        let fields = json!({
            "attachments": [
                "bad-entry"
            ],
        });
        let err =
            build_wire_message(source, destination, "title", "content", Some(fields), &signer)
                .expect_err("invalid attachment entries must fail");
        assert!(err.to_string().contains("attachments must be objects with canonical shape"));
    }

    #[test]
    fn build_wire_message_rejects_legacy_attachment_aliases() {
        let signer = PrivateIdentity::new_from_name("runtime-legacy-aliases");
        let source = [0x70; 16];
        let destination = [0x80; 16];
        let err = build_wire_message(
            source,
            destination,
            "title",
            "content",
            Some(json!({
                "files": [
                    {
                        "name": "bad.bin",
                        "data": [1, 2, 3]
                    }
                ]
            })),
            &signer,
        )
        .expect_err("legacy files alias must fail");
        assert!(err.to_string().contains("legacy field 'files' is not allowed"));

        let err = build_wire_message(
            source,
            destination,
            "title",
            "content",
            Some(json!({
                "5": [
                    ["bad.bin", [1, 2, 3]]
                ]
            })),
            &signer,
        )
        .expect_err("public field 5 must fail");
        assert!(err.to_string().contains("public field '5' is not allowed"));
    }

    #[test]
    fn fields_contain_attachments_from_sideband_metadata() {
        let fields = json!({
            "attachments": [
                {
                    "name": "legacy.txt",
                    "size": 3
                }
            ]
        });
        assert!(!can_send_opportunistic(Some(&fields), 1));
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

    #[test]
    fn rmpv_to_json_decodes_columba_meta_from_string() {
        let fields = rmpv::Value::Map(vec![
            (
                rmpv::Value::Integer(112_i64.into()),
                rmpv::Value::String(r#"{"sender":"alpha","type":"columba"}"#.into()),
            ),
            (
                rmpv::Value::Integer(113_i64.into()),
                rmpv::Value::String("fallback-text".to_string().into()),
            ),
        ]);
        let decoded = rmpv_to_json(&fields).expect("decoded");

        assert_eq!(decoded["112"]["sender"], serde_json::json!("alpha"));
        assert_eq!(decoded["112"]["type"], serde_json::json!("columba"));
        assert_eq!(decoded["113"], serde_json::json!("fallback-text"));
    }

    #[test]
    fn rmpv_to_json_decodes_columba_meta_from_binary_json() {
        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::Integer(112_i64.into()),
            rmpv::Value::Binary(br#"{"sender":"beta","type":"columba"}"#.to_vec()),
        )]);
        let decoded = rmpv_to_json(&fields).expect("decoded");

        assert_eq!(decoded["112"]["sender"], serde_json::json!("beta"));
        assert_eq!(decoded["112"]["type"], serde_json::json!("columba"));
    }

    #[test]
    fn rmpv_to_json_decodes_columba_meta_from_binary_utf8_msgpack() {
        let packed = rmp_serde::to_vec(&rmpv::Value::Integer(77_i64.into())).expect("pack meta");
        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::Integer(112_i64.into()),
            rmpv::Value::Binary(packed),
        )]);
        let decoded = rmpv_to_json(&fields).expect("decoded");

        assert_eq!(decoded["112"], serde_json::json!(77));
    }

    #[test]
    fn rmpv_to_json_preserves_unparseable_columba_meta_from_binary() {
        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::Integer(112_i64.into()),
            rmpv::Value::Binary(vec![0xc4]),
        )]);
        let decoded = rmpv_to_json(&fields).expect("decoded");

        assert_eq!(decoded["112"], serde_json::json!([196]));
    }

    #[test]
    fn rmpv_to_json_decodes_telemetry_stream_from_string_payload() {
        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::Integer(3_i64.into()),
            rmpv::Value::String("\u{7f}".into()),
        )]);

        let decoded = rmpv_to_json(&fields).expect("decoded");
        assert_eq!(decoded["3"], serde_json::json!(127));
    }

    #[test]
    fn rmpv_to_json_preserves_nonbinary_telemetry_payload_as_string() {
        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::Integer(2_i64.into()),
            rmpv::Value::String("\u{0100}".into()),
        )]);

        let decoded = rmpv_to_json(&fields).expect("decoded");
        assert_eq!(decoded["2"], serde_json::json!("\u{0100}"));
    }

    #[test]
    fn rmpv_to_json_preserves_unparseable_telemetry_from_string_payload() {
        let fields = rmpv::Value::Map(vec![(
            rmpv::Value::String("3".into()),
            rmpv::Value::String("\u{0100}".into()),
        )]);

        let decoded = rmpv_to_json(&fields).expect("decoded");
        assert_eq!(decoded["3"], serde_json::json!("\u{0100}"));
    }

    #[test]
    fn json_to_rmpv_preserves_noncanonical_numeric_keys_as_strings() {
        let fields = serde_json::json!({
            "01": "leading-zero",
            "-01": "noncanonical-negative",
        });
        let converted = json_to_rmpv(&fields).expect("to rmpv");
        let decoded = rmpv_to_json(&converted).expect("decoded");

        assert_eq!(decoded["01"], serde_json::json!("leading-zero"));
        assert_eq!(decoded["-01"], serde_json::json!("noncanonical-negative"));
    }
}
