mod announce_helpers;
mod announce_rate_limit;
mod bootstrap;
mod config;
mod delivery_options;
mod identity_io;
mod inbound_helpers;
mod peer_cache;
mod propagation_link;
mod public_types;
mod receipt_flow;
mod receipt_helpers;
mod relay_helpers;
mod request_handlers;
mod rpc_helpers;
mod runtime_loop;
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
use crate::payload_fields::WireFields;
use crate::LxmfError;
use announce_helpers::{
    annotate_peer_records_with_announce_metadata, encode_delivery_display_name_app_data,
    encode_propagation_node_app_data,
};
use announce_rate_limit::trigger_rate_limited_announce;
use delivery_options::merge_outbound_delivery_options;
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
    normalize_relay_destination_hash, propagation_relay_candidates, short_hash_prefix,
    wait_for_external_relay_selection,
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
use rpc_helpers::{annotate_response_meta, build_send_params_with_source, resolve_transport};
use runtime_loop::runtime_thread;
use send_helpers::{
    can_send_opportunistic, opportunistic_payload, parse_delivery_method, send_outcome_is_sent,
    send_outcome_status, DeliveryMethod,
};
use serde::Deserialize;
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
use std::time::{Duration, Instant};
use support::{
    clean_non_empty, extract_identity_hash, generate_message_id, interface_to_rpc, now_epoch_secs,
    parse_bind_host_port, source_hash_from_private_key_hex,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::watch;
use wire_codec::{build_wire_message, sanitize_outbound_wire_fields};
#[cfg(test)]
use wire_codec::{json_to_rmpv, rmpv_to_json};

pub use config::RuntimeConfig;
pub use public_types::{
    EventsProbeReport, RpcProbeReport, RuntimeProbeReport, SendCommandRequest, SendMessageRequest,
    SendMessageResponse,
};

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

        let result = self.call("send_message_v2", Some(params))?;

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

#[cfg(test)]
mod tests;
