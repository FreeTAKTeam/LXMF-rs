use serde::de::Visitor;
use serde::ser::SerializeMap;
use std::fmt;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct RpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct RpcResponse {
    pub id: u64,
    pub result: Option<JsonValue>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SdkCustomOperationSpec {
    pub id: String,
    pub group: String,
    pub kind: String,
    pub transport_variant: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

impl SdkCustomOperationSpec {
    pub fn new(
        id: impl Into<String>,
        group: impl Into<String>,
        kind: impl Into<String>,
        transport_variant: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: Self::trimmed(id),
            group: Self::trimmed(group),
            kind: Self::trimmed(kind).to_ascii_lowercase(),
            transport_variant: Self::trimmed(transport_variant),
            description: Self::trimmed(description),
            aliases: Vec::new(),
            required_capabilities: Vec::new(),
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        let alias = Self::trimmed(alias);
        if !alias.is_empty() && !self.aliases.iter().any(|current| current == &alias) {
            self.aliases.push(alias);
        }
        self
    }

    pub fn with_required_capability(mut self, capability: impl Into<String>) -> Self {
        let capability = Self::trimmed(capability);
        if !capability.is_empty()
            && !self.required_capabilities.iter().any(|current| current == &capability)
        {
            self.required_capabilities.push(capability);
        }
        self
    }

    fn trimmed(value: impl Into<String>) -> String {
        value.into().trim().to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Eq)]
pub struct SdkCursorHint {
    pub method: String,
    pub next_cursor: String,
    pub captured_at_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct RpcError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub machine_code: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub retryable: Option<bool>,
    #[serde(default)]
    pub is_user_actionable: Option<bool>,
    #[serde(default)]
    pub details: Option<Box<JsonMap<String, JsonValue>>>,
    #[serde(default)]
    pub cause_code: Option<String>,
    #[serde(default)]
    pub extensions: Option<Box<JsonMap<String, JsonValue>>>,
}

impl RpcError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        let message = message.into();
        let category = Self::category_for_code(code.as_str());
        let retryable =
            category.as_deref().is_some_and(|value| value == "Transport" || value == "Timeout");
        let is_user_actionable = category.as_deref().is_some_and(|value| {
            matches!(value, "Validation" | "Capability" | "Config" | "Policy" | "Security")
        });
        let machine_code = code.starts_with("SDK_").then_some(code.clone());
        Self {
            code,
            message,
            machine_code,
            category,
            retryable: Some(retryable),
            is_user_actionable: Some(is_user_actionable),
            details: None,
            cause_code: None,
            extensions: None,
        }
    }

    fn category_for_code(code: &str) -> Option<String> {
        if code.contains("_VALIDATION_") {
            return Some("Validation".to_string());
        }
        if code.contains("_CAPABILITY_") {
            return Some("Capability".to_string());
        }
        if code.contains("_CONFIG_") {
            return Some("Config".to_string());
        }
        if code.contains("_POLICY_") {
            return Some("Policy".to_string());
        }
        if code.contains("_TRANSPORT_") {
            return Some("Transport".to_string());
        }
        if code.contains("_STORAGE_") {
            return Some("Storage".to_string());
        }
        if code.contains("_CRYPTO_") {
            return Some("Crypto".to_string());
        }
        if code.contains("_TIMEOUT_") {
            return Some("Timeout".to_string());
        }
        if code.contains("_RUNTIME_") {
            return Some("Runtime".to_string());
        }
        if code.contains("_SECURITY_") {
            return Some("Security".to_string());
        }
        if code.contains("INTERNAL") {
            return Some("Internal".to_string());
        }
        None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct InterfaceRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct DeliveryPolicy {
    pub auth_required: bool,
    pub allowed_destinations: Vec<String>,
    pub denied_destinations: Vec<String>,
    pub ignored_destinations: Vec<String>,
    pub prioritised_destinations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PropagationState {
    pub enabled: bool,
    pub store_root: Option<String>,
    pub target_cost: u32,
    #[serde(default = "default_propagation_stamp_cost_flexibility")]
    pub stamp_cost_flexibility: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_storage_limit_mb: Option<u64>,
    #[serde(default = "default_delivery_transfer_limit")]
    pub delivery_limit: u32,
    #[serde(default = "default_propagation_transfer_limit")]
    pub propagation_limit: u32,
    #[serde(default = "default_propagation_sync_limit")]
    pub sync_limit: u32,
    #[serde(default = "default_true")]
    pub autopeer: bool,
    #[serde(default = "default_autopeer_maxdepth")]
    pub autopeer_maxdepth: u32,
    #[serde(default)]
    pub static_peers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_peers: Option<u32>,
    #[serde(default)]
    pub from_static_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peering_cost: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_peering_cost_max: Option<u32>,
    pub total_ingested: usize,
    pub last_ingest_count: usize,
    pub sync_state: u32,
    pub state_name: String,
    pub sync_progress: f64,
    pub messages_received: usize,
    pub max_messages: usize,
    #[serde(default)]
    pub client_propagation_messages_received: usize,
    #[serde(default)]
    pub client_propagation_messages_served: usize,
    #[serde(default)]
    pub unpeered_propagation_incoming: usize,
    #[serde(default)]
    pub unpeered_propagation_rx_bytes: u64,
    pub selected_node: Option<String>,
    pub last_sync_started: Option<i64>,
    pub last_sync_completed: Option<i64>,
    pub last_sync_error: Option<String>,
}

impl Default for PropagationState {
    fn default() -> Self {
        Self {
            enabled: false,
            store_root: None,
            target_cost: 0,
            stamp_cost_flexibility: default_propagation_stamp_cost_flexibility(),
            message_storage_limit_mb: None,
            delivery_limit: default_delivery_transfer_limit(),
            propagation_limit: default_propagation_transfer_limit(),
            sync_limit: default_propagation_sync_limit(),
            autopeer: default_true(),
            autopeer_maxdepth: default_autopeer_maxdepth(),
            static_peers: Vec::new(),
            max_peers: None,
            from_static_only: false,
            peering_cost: None,
            remote_peering_cost_max: None,
            total_ingested: 0,
            last_ingest_count: 0,
            sync_state: 0,
            state_name: String::new(),
            sync_progress: 0.0,
            messages_received: 0,
            max_messages: 0,
            client_propagation_messages_received: 0,
            client_propagation_messages_served: 0,
            unpeered_propagation_incoming: 0,
            unpeered_propagation_rx_bytes: 0,
            selected_node: None,
            last_sync_started: None,
            last_sync_completed: None,
            last_sync_error: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct StampPolicy {
    pub target_cost: u32,
    pub flexibility: u32,
    #[serde(default = "default_stamp_enforce")]
    pub enforce: bool,
}

impl Default for StampPolicy {
    fn default() -> Self {
        Self { target_cost: 0, flexibility: 0, enforce: default_stamp_enforce() }
    }
}

fn default_stamp_enforce() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Default)]
struct DaemonStatusSnapshot {
    peer_count: usize,
    interfaces: Vec<InterfaceRecord>,
    delivery_policy: DeliveryPolicy,
    propagation: PropagationState,
    stamp_policy: StampPolicy,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TicketRecord {
    pub destination: String,
    pub ticket: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DeliveryTraceEntry {
    pub status: String,
    pub timestamp: i64,
    #[serde(default)]
    pub reason_code: Option<String>,
}

const RPC_METRIC_LATENCY_BUCKETS_MS: [u64; 10] = [1, 5, 10, 25, 50, 100, 250, 500, 1_000, 5_000];

#[derive(Debug, Clone)]
struct RpcLatencyHistogram {
    bucket_counts: [u64; RPC_METRIC_LATENCY_BUCKETS_MS.len()],
    overflow_count: u64,
    count: u64,
    sum_ms: u64,
    max_ms: u64,
}

impl Default for RpcLatencyHistogram {
    fn default() -> Self {
        Self {
            bucket_counts: [0; RPC_METRIC_LATENCY_BUCKETS_MS.len()],
            overflow_count: 0,
            count: 0,
            sum_ms: 0,
            max_ms: 0,
        }
    }
}

impl RpcLatencyHistogram {
    fn observe(&mut self, value_ms: u64) {
        self.count = self.count.saturating_add(1);
        self.sum_ms = self.sum_ms.saturating_add(value_ms);
        self.max_ms = self.max_ms.max(value_ms);
        if let Some((idx, _)) = RPC_METRIC_LATENCY_BUCKETS_MS
            .iter()
            .enumerate()
            .find(|(_, bound_ms)| value_ms <= **bound_ms)
        {
            self.bucket_counts[idx] = self.bucket_counts[idx].saturating_add(1);
            return;
        }
        self.overflow_count = self.overflow_count.saturating_add(1);
    }

    fn as_json(&self) -> JsonValue {
        let buckets = RPC_METRIC_LATENCY_BUCKETS_MS
            .iter()
            .enumerate()
            .map(|(idx, bound_ms)| {
                json!({
                    "le_ms": bound_ms,
                    "count": self.bucket_counts[idx],
                })
            })
            .collect::<Vec<_>>();
        json!({
            "count": self.count,
            "sum_ms": self.sum_ms,
            "max_ms": self.max_ms,
            "overflow_count": self.overflow_count,
            "buckets": buckets,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct RpcMetrics {
    http_requests_total: u64,
    http_request_errors_total: u64,
    rpc_requests_total: u64,
    rpc_errors_total: u64,
    sdk_send_total: u64,
    sdk_send_success_total: u64,
    sdk_send_error_total: u64,
    sdk_poll_total: u64,
    sdk_poll_events_total: u64,
    sdk_poll_batches_with_gap_total: u64,
    sdk_cancel_total: u64,
    sdk_cancel_accepted_total: u64,
    sdk_cancel_too_late_total: u64,
    sdk_cancel_not_found_total: u64,
    sdk_cancel_already_terminal_total: u64,
    sdk_event_drops_total: u64,
    sdk_event_sink_publish_total: u64,
    sdk_event_sink_error_total: u64,
    sdk_event_sink_skipped_total: u64,
    sdk_auth_failures_total: u64,
    ble_connect_failures_total: u64,
    ble_chunk_retries_total: u64,
    ble_nacks_total: u64,
    ble_tx_queue_timeout_total: u64,
    attachment_upload_offset_reject_total: u64,
    attachment_upload_checksum_mismatch_total: u64,
    capture_success_total: u64,
    capture_failure_total: u64,
    http_requests_by_route: BTreeMap<String, u64>,
    rpc_requests_by_method: BTreeMap<String, u64>,
    rpc_errors_by_method: BTreeMap<String, u64>,
    sdk_event_sink_publish_by_kind: BTreeMap<String, u64>,
    sdk_event_sink_errors_by_kind: BTreeMap<String, u64>,
    ble_connect_failures_by_iface: BTreeMap<String, u64>,
    ble_chunk_retries_by_iface_reason: BTreeMap<String, u64>,
    ble_nacks_by_iface: BTreeMap<String, u64>,
    ble_tx_queue_timeout_by_iface: BTreeMap<String, u64>,
    attachment_upload_offset_reject_by_code: BTreeMap<String, u64>,
    capture_success_by_camera_id: BTreeMap<String, u64>,
    capture_failure_by_camera_reason: BTreeMap<String, u64>,
    sdk_send_latency_ms: RpcLatencyHistogram,
    sdk_poll_latency_ms: RpcLatencyHistogram,
    sdk_auth_latency_ms: RpcLatencyHistogram,
    sdk_send_store_write_ns_total: u64,
    sdk_send_store_write_ops_total: u64,
    sdk_send_delivery_schedule_ns_total: u64,
    sdk_send_delivery_schedule_ops_total: u64,
    sdk_send_event_publish_ns_total: u64,
    sdk_send_event_publish_ops_total: u64,
    daemon_status_lock_wait_ns_total: u64,
    daemon_status_snapshot_wait_ns_total: u64,
    daemon_status_message_count_wait_ns_total: u64,
    daemon_status_calls_total: u64,
    sdk_poll_event_log_lock_wait_ns_total: u64,
    sdk_poll_event_log_lock_ops_total: u64,
}

enum EventSinkCommand {
    Publish {
        sink: Arc<dyn EventSinkBridge>,
        sink_kind: String,
        envelope: RpcEventSinkEnvelope,
    },
    #[cfg(test)]
    Flush {
        reply: mpsc::Sender<()>,
    },
}

struct OutboundDeliveryCommand {
    record: MessageRecord,
    options: OutboundDeliveryOptions,
}

pub struct RpcDaemon {
    store: Arc<MessagesStore>,
    identity_hash: String,
    delivery_destination_hash: Mutex<Option<String>>,
    events: broadcast::Sender<RpcEvent>,
    sdk_events: broadcast::Sender<SequencedRpcEvent>,
    event_queue: Mutex<VecDeque<RpcEvent>>,
    sdk_event_log: Mutex<VecDeque<SequencedRpcEvent>>,
    sdk_next_event_seq: Mutex<u64>,
    announce_next_seq: Mutex<u64>,
    sdk_dropped_event_count: Mutex<u64>,
    sdk_active_contract_version: Mutex<u16>,
    sdk_profile: Mutex<String>,
    sdk_config_revision: Mutex<u64>,
    sdk_runtime_config: Mutex<JsonValue>,
    sdk_config_apply_lock: Mutex<()>,
    sdk_effective_capabilities: Mutex<Vec<String>>,
    sdk_custom_operations: Mutex<Vec<SdkCustomOperationSpec>>,
    sdk_stream_degraded: Mutex<bool>,
    sdk_seen_jti: Mutex<HashMap<String, u64>>,
    sdk_rate_window_started_ms: Mutex<u64>,
    sdk_rate_ip_counts: Mutex<HashMap<String, u32>>,
    sdk_rate_principal_counts: Mutex<HashMap<String, u32>>,
    sdk_domain_state_lock: Mutex<()>,
    sdk_next_domain_seq: Mutex<u64>,
    sdk_topics: Mutex<HashMap<String, SdkTopicRecord>>,
    sdk_topic_order: Mutex<Vec<String>>,
    sdk_topic_subscriptions: Mutex<HashSet<String>>,
    sdk_telemetry_points: Mutex<Vec<SdkTelemetryPoint>>,
    sdk_attachments: Mutex<HashMap<String, SdkAttachmentRecord>>,
    sdk_attachment_payloads: Mutex<HashMap<String, String>>,
    sdk_attachment_order: Mutex<Vec<String>>,
    sdk_attachment_uploads: Mutex<HashMap<String, SdkAttachmentUploadSession>>,
    sdk_cursor_hints: Mutex<HashMap<String, SdkCursorHint>>,
    sdk_markers: Mutex<HashMap<String, SdkMarkerRecord>>,
    sdk_marker_order: Mutex<Vec<String>>,
    sdk_identities: Mutex<HashMap<String, SdkIdentityBundle>>,
    sdk_contacts: Mutex<HashMap<String, SdkContactRecord>>,
    sdk_contact_order: Mutex<Vec<String>>,
    sdk_active_identity: Mutex<Option<String>>,
    sdk_remote_commands: Mutex<HashMap<String, SdkRemoteCommandRecord>>,
    sdk_voice_sessions: Mutex<HashMap<String, SdkVoiceSessionRecord>>,
    peers: Mutex<HashMap<String, PeerRecord>>,
    interfaces: Mutex<Vec<InterfaceRecord>>,
    delivery_policy: Mutex<DeliveryPolicy>,
    propagation_state: Mutex<PropagationState>,
    propagation_payloads: Mutex<HashMap<String, String>>,
    throttled_propagation_peers: Mutex<HashMap<String, i64>>,
    outbound_propagation_node: Mutex<Option<String>>,
    paper_ingest_seen: Mutex<HashSet<String>>,
    stamp_policy: Mutex<StampPolicy>,
    ticket_cache: Mutex<HashMap<String, TicketRecord>>,
    ticket_last_deliveries: Mutex<HashMap<String, i64>>,
    delivery_traces: Arc<Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>>,
    daemon_status_snapshot: std::sync::RwLock<DaemonStatusSnapshot>,
    delivery_status_lock: Arc<Mutex<()>>,
    sdk_metrics: Arc<Mutex<RpcMetrics>>,
    outbound_bridge: Option<Arc<dyn OutboundBridge>>,
    outbound_delivery_tx: Option<mpsc::SyncSender<OutboundDeliveryCommand>>,
    announce_bridge: Option<Arc<dyn AnnounceBridge>>,
    event_sink_bridges: Vec<Arc<dyn EventSinkBridge>>,
    event_sink_tx: Option<mpsc::SyncSender<EventSinkCommand>>,
    interface_mutation_bridge: Mutex<Option<Arc<dyn InterfaceMutationBridge>>>,
    remote_control_bridge: Mutex<Option<Arc<dyn RemoteControlBridge>>>,
    started_at: std::time::Instant,
}

pub trait OutboundBridge: Send + Sync {
    fn validate_delivery(
        &self,
        _record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn deliver(
        &self,
        record: &MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error>;

    fn encode_paper(
        &self,
        _record: &MessageRecord,
    ) -> Result<Option<PaperEncodeEnvelope>, std::io::Error> {
        Ok(None)
    }

    fn decode_paper_uri(&self, _uri: &str) -> Result<Option<PaperDecodeOutcome>, std::io::Error> {
        Ok(None)
    }

    fn delivery_pipeline_status(&self) -> Option<JsonValue> {
        None
    }
}

pub trait AnnounceBridge: Send + Sync {
    fn announce_now(&self) -> Result<(), std::io::Error>;
}

pub trait InterfaceMutationBridge: Send + Sync {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, std::io::Error>;
}

pub trait RemoteControlBridge: Send + Sync {
    fn propagation_remote_status(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error>;

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error>;

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error>;

    fn propagation_remote_download(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error>;

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RpcEventSinkEnvelope {
    pub contract_release: String,
    pub runtime_id: String,
    pub stream_id: String,
    pub seq_no: u64,
    pub emitted_at_ms: i64,
    pub event: RpcEvent,
}

pub trait EventSinkBridge: Send + Sync {
    fn sink_id(&self) -> &str;
    fn sink_kind(&self) -> &'static str;
    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> Result<(), std::io::Error>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct OutboundDeliveryOptions {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub stamp_cost: Option<u32>,
    #[serde(default)]
    pub include_ticket: bool,
    #[serde(default)]
    pub try_propagation_on_fail: bool,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub source_private_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PaperEncodeEnvelope {
    pub uri: String,
    pub transient_id: String,
    pub destination_hint: String,
    pub extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaperDecodeOutcome {
    pub transient_id: String,
    pub destination_hint: String,
    pub record: Option<MessageRecord>,
    pub raw_lxmf_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RpcEvent {
    pub event_type: String,
    pub payload: JsonValue,
}

#[derive(Debug, Clone)]
pub struct SequencedRpcEvent {
    pub seq_no: u64,
    pub event: RpcEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeerRecord {
    pub peer: String,
    pub last_seen: i64,
    pub capabilities: Vec<String>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub metadata: JsonValue,
    pub peer_type: Option<String>,
    pub alive: bool,
    pub last_sync_attempt: i64,
    pub next_sync_attempt: i64,
    pub sync_backoff: u32,
    pub network_distance: u32,
    pub offered: u64,
    pub outgoing: u64,
    pub incoming: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub sync_transfer_rate: f64,
    pub acceptance_rate: f64,
    pub first_seen: i64,
    pub seen_count: u64,
    pub peering_timebase: i64,
    pub sync_strategy: u8,
    pub propagation_transfer_limit: Option<u32>,
    pub propagation_sync_limit: Option<u32>,
    pub propagation_stamp_cost: Option<u32>,
    pub propagation_stamp_cost_flexibility: Option<u32>,
    pub peering_cost: Option<u32>,
    pub peering_key_stamp: Option<Vec<u8>>,
    pub peering_key_value: Option<u32>,
    pub restored_handled_ids: Vec<String>,
    pub restored_unhandled_ids: Vec<String>,
}

impl serde::Serialize for PeerRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("peer", &self.peer)?;
        map.serialize_entry("destination_hash", &self.peer)?;
        map.serialize_entry("last_seen", &self.last_seen)?;
        map.serialize_entry("last_heard", &self.last_seen)?;
        map.serialize_entry("capabilities", &self.capabilities)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("name_source", &self.name_source)?;
        map.serialize_entry("metadata", &self.metadata)?;
        map.serialize_entry("peer_type", &self.peer_type)?;
        map.serialize_entry("alive", &self.alive)?;
        map.serialize_entry("last_sync_attempt", &self.last_sync_attempt)?;
        map.serialize_entry("next_sync_attempt", &self.next_sync_attempt)?;
        map.serialize_entry("sync_backoff", &self.sync_backoff)?;
        map.serialize_entry("network_distance", &self.network_distance)?;
        map.serialize_entry("offered", &self.offered)?;
        map.serialize_entry("outgoing", &self.outgoing)?;
        map.serialize_entry("incoming", &self.incoming)?;
        map.serialize_entry("rx_bytes", &self.rx_bytes)?;
        map.serialize_entry("tx_bytes", &self.tx_bytes)?;
        map.serialize_entry("sync_transfer_rate", &self.sync_transfer_rate)?;
        map.serialize_entry("str", &self.sync_transfer_rate)?;
        map.serialize_entry("acceptance_rate", &self.acceptance_rate)?;
        map.serialize_entry("first_seen", &self.first_seen)?;
        map.serialize_entry("seen_count", &self.seen_count)?;
        map.serialize_entry("peering_timebase", &self.peering_timebase)?;
        map.serialize_entry("sync_strategy", &self.sync_strategy)?;
        if let Some(value) = self.propagation_transfer_limit {
            map.serialize_entry("propagation_transfer_limit", &bytes_to_kilobytes(value))?;
            map.serialize_entry("transfer_limit", &value)?;
        }
        if let Some(value) = self.propagation_sync_limit {
            map.serialize_entry(
                "propagation_sync_limit",
                &bytes_to_python_sync_limit_kilobytes(value),
            )?;
            map.serialize_entry("sync_limit", &value)?;
        }
        if let Some(value) = self.propagation_stamp_cost {
            map.serialize_entry("propagation_stamp_cost", &value)?;
            map.serialize_entry("target_stamp_cost", &value)?;
        }
        if let Some(value) = self.propagation_stamp_cost_flexibility {
            map.serialize_entry("propagation_stamp_cost_flexibility", &value)?;
            map.serialize_entry("stamp_cost_flexibility", &value)?;
        }
        if let Some(value) = self.peering_cost {
            map.serialize_entry("peering_cost", &value)?;
        }
        if let (Some(stamp), Some(value)) = (&self.peering_key_stamp, self.peering_key_value) {
            map.serialize_entry(
                "peering_key",
                &(serde_bytes::Bytes::new(stamp.as_slice()), value),
            )?;
        }
        map.serialize_entry("handled_ids", &self.restored_handled_ids)?;
        map.serialize_entry("unhandled_ids", &self.restored_unhandled_ids)?;
        map.end()
    }
}

#[derive(Deserialize)]
struct PeerRecordWire {
    #[serde(default)]
    peer: Option<PythonHexId>,
    #[serde(default)]
    destination_hash: Option<PythonHexId>,
    #[serde(default)]
    last_seen: Option<JsonValue>,
    #[serde(default)]
    last_heard: Option<JsonValue>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    name_source: Option<String>,
    #[serde(default)]
    metadata: JsonValue,
    #[serde(default)]
    peer_type: Option<String>,
    #[serde(default)]
    alive: bool,
    #[serde(default)]
    last_sync_attempt: Option<JsonValue>,
    #[serde(default)]
    next_sync_attempt: Option<JsonValue>,
    #[serde(default)]
    sync_backoff: u32,
    #[serde(default = "default_network_distance")]
    network_distance: u32,
    #[serde(default)]
    offered: Option<JsonValue>,
    #[serde(default)]
    outgoing: Option<JsonValue>,
    #[serde(default)]
    incoming: Option<JsonValue>,
    #[serde(default)]
    rx_bytes: Option<JsonValue>,
    #[serde(default)]
    tx_bytes: Option<JsonValue>,
    #[serde(default)]
    sync_transfer_rate: Option<f64>,
    #[serde(default)]
    str: Option<f64>,
    #[serde(default)]
    acceptance_rate: Option<f64>,
    #[serde(default)]
    first_seen: Option<i64>,
    #[serde(default)]
    seen_count: Option<u64>,
    #[serde(default)]
    peering_timebase: Option<JsonValue>,
    #[serde(default)]
    sync_strategy: Option<JsonValue>,
    #[serde(default)]
    propagation_transfer_limit: Option<JsonValue>,
    #[serde(default)]
    transfer_limit: Option<JsonValue>,
    #[serde(default)]
    propagation_sync_limit: Option<JsonValue>,
    #[serde(default)]
    sync_limit: Option<JsonValue>,
    #[serde(default)]
    propagation_stamp_cost: Option<JsonValue>,
    #[serde(default)]
    target_stamp_cost: Option<JsonValue>,
    #[serde(default)]
    propagation_stamp_cost_flexibility: Option<JsonValue>,
    #[serde(default)]
    stamp_cost_flexibility: Option<JsonValue>,
    #[serde(default)]
    peering_cost: Option<JsonValue>,
    #[serde(default)]
    peering_key: Option<PythonPeeringKey>,
    #[serde(default)]
    handled_ids: Vec<PythonHexId>,
    #[serde(default)]
    unhandled_ids: Vec<PythonHexId>,
}

impl<'de> Deserialize<'de> for PeerRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PeerRecordWire::deserialize(deserializer)?;
        let peer = wire
            .peer
            .map(PythonHexId::into_string)
            .or_else(|| wire.destination_hash.map(PythonHexId::into_string))
            .ok_or_else(|| serde::de::Error::missing_field("peer"))?;
        let last_seen = if let Some(value) = wire.last_seen.as_ref() {
            parse_python_timestamp_i64(value).map_err(serde::de::Error::custom)?
        } else if let Some(value) = wire.last_heard.as_ref() {
            parse_python_timestamp_i64(value).map_err(serde::de::Error::custom)?
        } else {
            return Err(serde::de::Error::missing_field("last_seen"));
        };
        let last_sync_attempt = wire
            .last_sync_attempt
            .as_ref()
            .map(parse_python_timestamp_i64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let next_sync_attempt = wire
            .next_sync_attempt
            .as_ref()
            .map(parse_python_timestamp_i64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let peering_timebase = wire
            .peering_timebase
            .as_ref()
            .map(parse_python_timestamp_i64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let sync_transfer_rate = wire.sync_transfer_rate.or(wire.str).unwrap_or_default();
        let offered = wire.offered.as_ref().and_then(parse_python_int_u64).unwrap_or_default();
        let outgoing = wire
            .outgoing
            .as_ref()
            .and_then(parse_python_int_u64)
            .unwrap_or_default();
        let incoming = wire
            .incoming
            .as_ref()
            .and_then(parse_python_int_u64)
            .unwrap_or_default();
        let rx_bytes = wire
            .rx_bytes
            .as_ref()
            .and_then(parse_python_int_u64)
            .unwrap_or_default();
        let tx_bytes = wire
            .tx_bytes
            .as_ref()
            .and_then(parse_python_int_u64)
            .unwrap_or_default();
        let acceptance_rate = wire.acceptance_rate.unwrap_or_else(|| {
            if offered == 0 {
                0.0
            } else {
                (outgoing as f64 / offered as f64).max(0.0)
            }
        });
        let python_transfer_limit = wire.propagation_transfer_limit.is_some();
        let transfer_limit = parse_peer_limit_bytes(
            wire.propagation_transfer_limit.as_ref(),
            wire.transfer_limit.as_ref(),
            python_transfer_limit,
        );
        let python_sync_limit = wire.propagation_sync_limit.is_some();
        let sync_limit = parse_peer_sync_limit_bytes(
            wire.propagation_sync_limit.as_ref(),
            wire.sync_limit.as_ref(),
            python_sync_limit,
        )
        .or_else(|| python_transfer_limit.then_some(transfer_limit).flatten());
        let (peering_key_stamp, peering_key_value) =
            wire.peering_key.map(PythonPeeringKey::into_parts).unwrap_or_default();
        Ok(Self {
            peer,
            last_seen,
            capabilities: wire.capabilities,
            name: wire.name,
            name_source: wire.name_source,
            metadata: wire.metadata,
            peer_type: wire.peer_type,
            alive: wire.alive,
            last_sync_attempt,
            next_sync_attempt,
            sync_backoff: wire.sync_backoff,
            network_distance: wire.network_distance,
            offered,
            outgoing,
            incoming,
            rx_bytes,
            tx_bytes,
            sync_transfer_rate,
            acceptance_rate,
            first_seen: wire.first_seen.unwrap_or(last_seen),
            seen_count: wire.seen_count.unwrap_or_else(|| u64::from(last_seen > 0)),
            peering_timebase,
            sync_strategy: wire
                .sync_strategy
                .as_ref()
                .and_then(parse_python_int_u8)
                .unwrap_or_else(default_peer_sync_strategy),
            propagation_transfer_limit: transfer_limit,
            propagation_sync_limit: sync_limit,
            propagation_stamp_cost: wire
                .propagation_stamp_cost
                .as_ref()
                .and_then(parse_python_int_u32)
                .or_else(|| wire.target_stamp_cost.as_ref().and_then(parse_python_int_u32)),
            propagation_stamp_cost_flexibility: wire
                .propagation_stamp_cost_flexibility
                .as_ref()
                .and_then(parse_python_int_u32)
                .or_else(|| {
                    wire.stamp_cost_flexibility
                        .as_ref()
                        .and_then(parse_python_int_u32)
                }),
            peering_cost: wire.peering_cost.as_ref().and_then(parse_python_int_u32),
            peering_key_stamp,
            peering_key_value,
            restored_handled_ids: wire
                .handled_ids
                .into_iter()
                .map(PythonHexId::into_string)
                .collect(),
            restored_unhandled_ids: wire
                .unhandled_ids
                .into_iter()
                .map(PythonHexId::into_string)
                .collect(),
        })
    }
}

struct PythonHexId(String);

impl PythonHexId {
    fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for PythonHexId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PythonHexIdVisitor;

        impl Visitor<'_> for PythonHexIdVisitor {
            type Value = PythonHexId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a hex string or MessagePack binary hash")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonHexId(value.trim().to_ascii_lowercase()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value.as_str())
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonHexId(hex::encode(value)))
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_bytes(value.as_slice())
            }
        }

        deserializer.deserialize_any(PythonHexIdVisitor)
    }
}

struct PythonPeeringKey {
    stamp: Option<Vec<u8>>,
    value: Option<u32>,
}

impl PythonPeeringKey {
    fn value(value: Option<u32>) -> Self {
        Self { stamp: None, value }
    }

    fn into_parts(self) -> (Option<Vec<u8>>, Option<u32>) {
        (self.stamp, self.value)
    }
}

impl<'de> Deserialize<'de> for PythonPeeringKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PythonPeeringKeyVisitor;

        impl<'de> Visitor<'de> for PythonPeeringKeyVisitor {
            type Value = PythonPeeringKey;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a peering key value or [stamp, value] pair")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(PythonPeeringKey::value(u32::try_from(value).ok()))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(PythonPeeringKey::value(u32::try_from(value.max(0)).ok()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                let value = value.max(0.0).floor();
                Ok(PythonPeeringKey::value(
                    (value.is_finite() && value <= f64::from(u32::MAX)).then_some(value as u32),
                ))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                let value = value.trim().parse::<f64>().ok().and_then(|value| {
                    let value = value.max(0.0).floor();
                    (value.is_finite() && value <= f64::from(u32::MAX)).then_some(value as u32)
                });
                Ok(PythonPeeringKey::value(value))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value.as_str())
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let stamp = sequence
                    .next_element::<PythonPeeringKeyStamp>()?
                    .and_then(PythonPeeringKeyStamp::into_bytes);
                let value = sequence.next_element::<JsonValue>()?;
                Ok(PythonPeeringKey {
                    stamp,
                    value: value.as_ref().and_then(parse_json_u32),
                })
            }
        }

        deserializer.deserialize_any(PythonPeeringKeyVisitor)
    }
}

struct PythonPeeringKeyStamp(Option<Vec<u8>>);

impl PythonPeeringKeyStamp {
    fn into_bytes(self) -> Option<Vec<u8>> {
        self.0
    }
}

impl<'de> Deserialize<'de> for PythonPeeringKeyStamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PythonPeeringKeyStampVisitor;

        impl<'de> Visitor<'de> for PythonPeeringKeyStampVisitor {
            type Value = PythonPeeringKeyStamp;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a nil, string, byte array, or MessagePack binary stamp")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(PythonPeeringKeyStamp(None))
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonPeeringKeyStamp(Some(value.to_vec())))
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonPeeringKeyStamp(Some(value)))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonPeeringKeyStamp(Some(value.as_bytes().to_vec())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value.as_str())
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(byte) = sequence.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(PythonPeeringKeyStamp(Some(bytes)))
            }
        }

        deserializer.deserialize_any(PythonPeeringKeyStampVisitor)
    }
}

fn parse_peer_limit_bytes(
    primary: Option<&JsonValue>,
    alias: Option<&JsonValue>,
    primary_is_python_kb: bool,
) -> Option<u32> {
    if let Some(alias) = alias {
        let alias_bytes = parse_json_u32(alias)?;
        if primary_is_python_kb {
            if let Some(primary) = primary {
                let Some(primary_kb) = parse_json_f64(primary) else {
                    return Some(alias_bytes);
                };
                if kilobytes_to_bytes(primary_kb) == Some(alias_bytes) {
                    return Some(alias_bytes);
                }
                if primary_kb == 0.0 && alias_bytes > 0 {
                    return parse_json_u32(primary);
                }
                return Some(alias_bytes);
            }
        }
        Some(alias_bytes)
    } else if primary_is_python_kb {
        parse_json_f64(primary?).and_then(kilobytes_to_bytes)
    } else {
        parse_json_u32(primary?)
    }
}

fn parse_peer_sync_limit_bytes(
    primary: Option<&JsonValue>,
    alias: Option<&JsonValue>,
    primary_is_python_kb: bool,
) -> Option<u32> {
    if let Some(alias) = alias {
        parse_json_u32(alias)
    } else if primary_is_python_kb {
        parse_python_sync_limit_bytes(primary?)
    } else {
        parse_json_u32(primary?)
    }
}

fn parse_json_u32(value: &JsonValue) -> Option<u32> {
    if let Some(value) = value.as_u64() {
        u32::try_from(value).ok()
    } else if let Some(value) = value.as_i64() {
        u32::try_from(value.max(0)).ok()
    } else {
        parse_json_f64(value).and_then(|value| {
            let bytes = value.max(0.0).floor();
            (bytes.is_finite() && bytes <= f64::from(u32::MAX)).then_some(bytes as u32)
        })
    }
}

fn parse_json_f64(value: &JsonValue) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn kilobytes_to_bytes(value: f64) -> Option<u32> {
    let bytes = (value.max(0.0) * 1000.0).floor();
    (bytes.is_finite() && bytes <= f64::from(u32::MAX)).then_some(bytes as u32)
}

fn parse_python_sync_limit_bytes(value: &JsonValue) -> Option<u32> {
    let kilobytes = f64::from(parse_python_int_u32(value)?);
    kilobytes_to_bytes(kilobytes)
}

fn parse_python_int_u32(value: &JsonValue) -> Option<u32> {
    if let Some(value) = value.as_u64() {
        u32::try_from(value).ok()
    } else if let Some(value) = value.as_i64() {
        u32::try_from(value.max(0)).ok()
    } else if let Some(value) = value.as_f64() {
        let value = value.max(0.0).trunc();
        (value.is_finite() && value <= f64::from(u32::MAX)).then_some(value as u32)
    } else if let Some(value) = value.as_bool() {
        Some(u32::from(value))
    } else if let Some(value) = value.as_str() {
        u32::try_from(value.trim().parse::<i64>().ok()?.max(0)).ok()
    } else {
        None
    }
}

fn parse_python_int_u64(value: &JsonValue) -> Option<u64> {
    if let Some(value) = value.as_u64() {
        Some(value)
    } else if let Some(value) = value.as_i64() {
        u64::try_from(value.max(0)).ok()
    } else if let Some(value) = value.as_f64() {
        let value = value.max(0.0).trunc();
        (value.is_finite() && value <= u64::MAX as f64).then_some(value as u64)
    } else if let Some(value) = value.as_bool() {
        Some(u64::from(value))
    } else if let Some(value) = value.as_str() {
        u64::try_from(value.trim().parse::<i64>().ok()?.max(0)).ok()
    } else {
        None
    }
}

fn parse_python_int_u8(value: &JsonValue) -> Option<u8> {
    u8::try_from(parse_python_int_u32(value)?).ok()
}

fn parse_python_timestamp_i64(value: &JsonValue) -> Result<i64, &'static str> {
    if let Some(value) = value.as_i64() {
        Ok(value)
    } else if let Some(value) = value.as_u64() {
        i64::try_from(value).map_err(|_| "timestamp exceeds i64 range")
    } else if let Some(value) = value.as_f64() {
        let value = value.trunc();
        if value.is_finite() && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
            Ok(value as i64)
        } else {
            Err("invalid timestamp")
        }
    } else {
        Err("invalid timestamp")
    }
}

fn bytes_to_kilobytes(value: u32) -> f64 {
    f64::from(value) / 1000.0
}

fn bytes_to_python_sync_limit_kilobytes(value: u32) -> u32 {
    if value == 0 {
        0
    } else {
        value.saturating_add(999) / 1000
    }
}

fn default_true() -> bool {
    true
}

fn default_autopeer_maxdepth() -> u32 {
    6
}

fn default_propagation_stamp_cost_flexibility() -> u32 {
    3
}

fn default_delivery_transfer_limit() -> u32 {
    1000
}

fn default_propagation_transfer_limit() -> u32 {
    256
}

fn default_propagation_sync_limit() -> u32 {
    10240
}

fn default_network_distance() -> u32 {
    1
}

fn default_peer_sync_strategy() -> u8 {
    2
}

#[cfg(test)]
mod peer_record_serde_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn peer_record_deserializes_legacy_seen_fields_from_last_seen() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-legacy",
            "last_seen": 1_700_001_001,
        }))
        .expect("deserialize legacy peer");

        assert_eq!(record.first_seen, 1_700_001_001);
        assert_eq!(record.seen_count, 1);
    }

    #[test]
    fn peer_record_deserializes_explicit_seen_fields_without_rewriting_them() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-current",
            "last_seen": 1_700_001_020,
            "first_seen": 1_700_001_000,
            "seen_count": 4,
        }))
        .expect("deserialize current peer");

        assert_eq!(record.first_seen, 1_700_001_000);
        assert_eq!(record.seen_count, 4);
    }

    #[test]
    fn peer_record_deserializes_unseen_legacy_peer_without_synthetic_seen_count() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-static",
            "last_seen": 0,
        }))
        .expect("deserialize unseen peer");

        assert_eq!(record.first_seen, 0);
        assert_eq!(record.seen_count, 0);
    }

    #[test]
    fn peer_record_deserializes_python_status_aliases() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-python-status",
            "last_heard": 1_700_001_004,
            "str": 4096.0,
            "offered": 7,
            "outgoing": 5,
            "incoming": 3,
            "transfer_limit": 333,
            "sync_limit": 444,
            "target_stamp_cost": 7,
            "stamp_cost_flexibility": 2,
        }))
        .expect("deserialize python status peer");

        assert_eq!(record.last_seen, 1_700_001_004);
        assert_eq!(record.first_seen, 1_700_001_004);
        assert_eq!(record.seen_count, 1);
        assert_eq!(record.sync_transfer_rate, 4096.0);
        assert_eq!(record.propagation_transfer_limit, Some(333));
        assert_eq!(record.propagation_sync_limit, Some(444));
        assert_eq!(record.propagation_stamp_cost, Some(7));
        assert_eq!(record.propagation_stamp_cost_flexibility, Some(2));
        let value = serde_json::to_value(record).expect("serialize python status peer");
        assert_eq!(value["offered"].as_u64(), Some(7));
        assert_eq!(value["outgoing"].as_u64(), Some(5));
        assert_eq!(value["incoming"].as_u64(), Some(3));
    }

    #[test]
    fn peer_record_deserializes_python_destination_hash_alias() {
        let record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-destination",
            "last_heard": 1_700_001_007,
            "sync_strategy": 2,
            "peering_key": ["not-used-in-rust", 3],
            "handled_ids": [],
            "unhandled_ids": [],
            "offered": 2,
            "outgoing": 1,
            "incoming": 4,
            "peering_cost": 3,
        }))
        .expect("deserialize python serialized peer");

        assert_eq!(record.peer, "peer-python-destination");
        assert_eq!(record.last_seen, 1_700_001_007);
        assert_eq!(record.first_seen, 1_700_001_007);
        assert_eq!(record.seen_count, 1);
        assert_eq!(record.offered, 2);
        assert_eq!(record.outgoing, 1);
        assert_eq!(record.incoming, 4);
        assert_eq!(record.peering_cost, Some(3));
        assert_eq!(
            record.peering_key_stamp,
            Some(b"not-used-in-rust".to_vec())
        );
        assert_eq!(record.peering_key_value, Some(3));
        assert_eq!(record.sync_strategy, 2);
    }

    #[test]
    fn peer_record_roundtrips_python_metadata_like_lxmpeer() {
        let record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-metadata",
            "last_heard": 1_700_001_009,
            "metadata": {
                "name": "Mesh Relay",
                "operator": "alpha"
            },
            "handled_ids": [],
            "unhandled_ids": [],
        }))
        .expect("deserialize python peer metadata");

        assert_eq!(record.metadata["name"].as_str(), Some("Mesh Relay"));
        let serialized = serde_json::to_value(&record).expect("serialize peer record");
        assert_eq!(serialized["metadata"]["operator"].as_str(), Some("alpha"));
    }

    #[test]
    fn peer_record_deserializes_python_msgpack_binary_peer_ids() {
        fn key(value: &str) -> rmpv::Value {
            rmpv::Value::String(value.into())
        }

        let destination_hash = (0x10_u8..0x20).collect::<Vec<_>>();
        let peering_key_stamp = vec![0xab; 32];
        let handled_id = (0x20_u8..0x40).collect::<Vec<_>>();
        let unhandled_id = (0x40_u8..0x60).collect::<Vec<_>>();
        let payload = rmpv::Value::Map(vec![
            (key("destination_hash"), rmpv::Value::Binary(destination_hash.clone())),
            (key("last_heard"), rmpv::Value::from(1_700_001_008_i64)),
            (key("sync_strategy"), rmpv::Value::from(2_u8)),
            (
                key("peering_key"),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(peering_key_stamp.clone()),
                    rmpv::Value::from(3_u8),
                ]),
            ),
            (key("handled_ids"), rmpv::Value::Array(vec![rmpv::Value::Binary(handled_id.clone())])),
            (
                key("unhandled_ids"),
                rmpv::Value::Array(vec![rmpv::Value::Binary(unhandled_id.clone())]),
            ),
            (key("offered"), rmpv::Value::from(2_u8)),
            (key("outgoing"), rmpv::Value::from(1_u8)),
            (key("incoming"), rmpv::Value::from(4_u8)),
            (key("peering_cost"), rmpv::Value::from(3_u8)),
        ]);
        let encoded = rmp_serde::to_vec(&payload).expect("encode python peer record");
        let record: PeerRecord =
            rmp_serde::from_slice(encoded.as_slice()).expect("deserialize python binary peer");

        assert_eq!(record.peer, hex::encode(destination_hash));
        assert_eq!(record.restored_handled_ids, vec![hex::encode(handled_id)]);
        assert_eq!(record.restored_unhandled_ids, vec![hex::encode(unhandled_id)]);
        assert_eq!(record.last_seen, 1_700_001_008);
        assert_eq!(record.peering_key_stamp, Some(peering_key_stamp.clone()));
        assert_eq!(record.peering_key_value, Some(3));

        let reencoded = rmp_serde::to_vec(&record).expect("serialize python binary peer");
        let reencoded: rmpv::Value =
            rmp_serde::from_slice(reencoded.as_slice()).expect("decode serialized peer");
        let rmpv::Value::Map(entries) = reencoded else {
            panic!("serialized peer should be a map");
        };
        let peering_key = entries
            .iter()
            .find_map(|(key, value)| {
                (key.as_str() == Some("peering_key")).then_some(value)
            })
            .expect("serialized peering key");
        let rmpv::Value::Array(items) = peering_key else {
            panic!("serialized peering key should be a pair");
        };
        assert_eq!(items.first(), Some(&rmpv::Value::Binary(peering_key_stamp)));
        assert_eq!(items.get(1).and_then(rmpv::Value::as_u64), Some(3));

        let peer_hash = (0xa0_u8..0xb0).collect::<Vec<_>>();
        let peer_payload = rmpv::Value::Map(vec![
            (key("peer"), rmpv::Value::Binary(peer_hash.clone())),
            (key("last_heard"), rmpv::Value::from(1_700_001_009_i64)),
            (key("handled_ids"), rmpv::Value::Array(Vec::new())),
            (key("unhandled_ids"), rmpv::Value::Array(Vec::new())),
        ]);
        let encoded = rmp_serde::to_vec(&peer_payload).expect("encode binary peer record");
        let record: PeerRecord =
            rmp_serde::from_slice(encoded.as_slice()).expect("deserialize binary peer field");
        assert_eq!(record.peer, hex::encode(peer_hash));
    }

    #[test]
    fn peer_record_derives_python_acceptance_rate_when_alias_is_absent() {
        let record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-acceptance",
            "last_heard": 1_700_001_008,
            "offered": 4,
            "outgoing": 1,
            "handled_ids": [],
            "unhandled_ids": [],
        }))
        .expect("deserialize python serialized peer");

        assert_eq!(record.acceptance_rate, 0.25);

        let duplicate_response_record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-duplicate-acceptance",
            "last_heard": 1_700_001_009,
            "offered": 1,
            "outgoing": 2,
            "handled_ids": [],
            "unhandled_ids": [],
        }))
        .expect("deserialize python serialized peer with duplicate-response counters");

        assert_eq!(duplicate_response_record.acceptance_rate, 2.0);
    }

    #[test]
    fn peer_record_deserializes_python_serialized_kilobyte_limits_as_runtime_bytes() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-python-serialized-limits",
            "last_seen": 1_700_001_004,
            "propagation_transfer_limit": 0.08,
            "propagation_sync_limit": 1,
        }))
        .expect("deserialize python serialized peer");

        assert_eq!(record.propagation_transfer_limit, Some(80));
        assert_eq!(record.propagation_sync_limit, Some(1_000));

        let transfer_only: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-python-serialized-transfer-only",
            "last_seen": 1_700_001_004,
            "propagation_transfer_limit": 0.152,
        }))
        .expect("deserialize python serialized peer with transfer limit only");

        assert_eq!(transfer_only.propagation_transfer_limit, Some(152));
        assert_eq!(transfer_only.propagation_sync_limit, Some(152));
    }

    #[test]
    fn peer_record_prefers_internal_status_fields_over_aliases() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-internal-status",
            "last_seen": 0,
            "last_heard": 1_700_001_004,
            "sync_transfer_rate": 0.0,
            "str": 4096.0,
            "propagation_transfer_limit": 0,
            "transfer_limit": 333,
        }))
        .expect("deserialize internal and alias status peer");

        assert_eq!(record.last_seen, 0);
        assert_eq!(record.first_seen, 0);
        assert_eq!(record.seen_count, 0);
        assert_eq!(record.sync_transfer_rate, 0.0);
        assert_eq!(record.propagation_transfer_limit, Some(0));
    }

    #[test]
    fn peer_record_serializes_python_status_aliases() {
        let record = PeerRecord {
            peer: "peer-python-status".to_string(),
            last_seen: 1_700_001_005,
            capabilities: vec!["propagation".to_string()],
            name: Some("Peer Python Status".to_string()),
            name_source: Some("announce".to_string()),
            metadata: JsonValue::Null,
            peer_type: Some("auto".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_000,
            next_sync_attempt: 1_700_001_720,
            sync_backoff: 720,
            network_distance: 3,
            offered: 7,
            outgoing: 5,
            incoming: 3,
            rx_bytes: 123,
            tx_bytes: 456,
            sync_transfer_rate: 2048.0,
            acceptance_rate: 0.5,
            first_seen: 1_700_000_900,
            seen_count: 4,
            peering_timebase: 1_700_000_950,
            sync_strategy: 2,
            propagation_transfer_limit: Some(333),
            propagation_sync_limit: Some(444),
            propagation_stamp_cost: Some(7),
            propagation_stamp_cost_flexibility: Some(2),
            peering_cost: Some(9),
            peering_key_stamp: Some(b"python-stamp".to_vec()),
            peering_key_value: Some(9),
            restored_handled_ids: vec!["aa".repeat(32), "bb".repeat(32)],
            restored_unhandled_ids: vec!["cc".repeat(32)],
        };

        let value = serde_json::to_value(&record).expect("serialize peer record");
        assert_eq!(value["destination_hash"].as_str(), Some("peer-python-status"));
        assert_eq!(value["last_seen"].as_i64(), Some(1_700_001_005));
        assert_eq!(value["last_heard"].as_i64(), Some(1_700_001_005));
        assert_eq!(value["sync_transfer_rate"].as_f64(), Some(2048.0));
        assert_eq!(value["str"].as_f64(), Some(2048.0));
        assert_eq!(value["offered"].as_u64(), Some(7));
        assert_eq!(value["outgoing"].as_u64(), Some(5));
        assert_eq!(value["incoming"].as_u64(), Some(3));
        assert_eq!(value["propagation_transfer_limit"].as_f64(), Some(0.333));
        assert_eq!(value["transfer_limit"].as_u64(), Some(333));
        assert_eq!(value["propagation_sync_limit"].as_u64(), Some(1));
        assert_eq!(value["sync_limit"].as_u64(), Some(444));
        assert_eq!(value["propagation_stamp_cost"].as_u64(), Some(7));
        assert_eq!(value["target_stamp_cost"].as_u64(), Some(7));
        assert_eq!(value["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
        assert_eq!(value["stamp_cost_flexibility"].as_u64(), Some(2));
        assert_eq!(
            value["peering_key"][0].as_array().map(|bytes| {
                bytes
                    .iter()
                    .map(|byte| byte.as_u64().expect("stamp byte") as u8)
                    .collect::<Vec<_>>()
            }),
            Some(b"python-stamp".to_vec())
        );
        assert_eq!(value["peering_key"][1].as_u64(), Some(9));
        assert_eq!(
            value["handled_ids"].as_array().expect("handled ids"),
            &[json!("aa".repeat(32)), json!("bb".repeat(32))]
        );
        assert_eq!(
            value["unhandled_ids"].as_array().expect("unhandled ids"),
            &[json!("cc".repeat(32))]
        );

        let without_stamp = PeerRecord {
            peering_key_stamp: None,
            ..record
        };
        let value = serde_json::to_value(without_stamp).expect("serialize peer without stamp");
        assert!(value.get("peering_key").is_none());
    }

    #[test]
    fn peer_record_serializes_python_limit_fields_as_kilobytes_with_byte_aliases() {
        let record = PeerRecord {
            peer: "peer-python-limits".to_string(),
            last_seen: 1_700_001_005,
            capabilities: vec!["propagation".to_string()],
            name: None,
            name_source: None,
            metadata: JsonValue::Null,
            peer_type: Some("auto".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_000,
            next_sync_attempt: 1_700_001_720,
            sync_backoff: 720,
            network_distance: 3,
            offered: 0,
            outgoing: 0,
            incoming: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            sync_transfer_rate: 0.0,
            acceptance_rate: 0.0,
            first_seen: 1_700_000_900,
            seen_count: 4,
            peering_timebase: 1_700_000_950,
            sync_strategy: 2,
            propagation_transfer_limit: Some(333),
            propagation_sync_limit: Some(444),
            propagation_stamp_cost: Some(7),
            propagation_stamp_cost_flexibility: Some(2),
            peering_cost: Some(9),
            peering_key_stamp: None,
            peering_key_value: None,
            restored_handled_ids: Vec::new(),
            restored_unhandled_ids: Vec::new(),
        };

        let value = serde_json::to_value(record).expect("serialize peer record");
        assert_eq!(value["propagation_transfer_limit"].as_f64(), Some(0.333));
        assert_eq!(value["transfer_limit"].as_u64(), Some(333));
        assert_eq!(value["propagation_sync_limit"].as_u64(), Some(1));
        assert_eq!(value["sync_limit"].as_u64(), Some(444));
    }

    #[test]
    fn peer_record_serializes_python_sync_limit_as_integer_kilobytes() {
        let record = PeerRecord {
            peer: "peer-python-sync-limit".to_string(),
            last_seen: 1_700_001_005,
            capabilities: vec!["propagation".to_string()],
            name: None,
            name_source: None,
            metadata: JsonValue::Null,
            peer_type: Some("auto".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_000,
            next_sync_attempt: 1_700_001_720,
            sync_backoff: 720,
            network_distance: 3,
            offered: 0,
            outgoing: 0,
            incoming: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            sync_transfer_rate: 0.0,
            acceptance_rate: 0.0,
            first_seen: 1_700_000_900,
            seen_count: 4,
            peering_timebase: 1_700_000_950,
            sync_strategy: 2,
            propagation_transfer_limit: Some(333),
            propagation_sync_limit: Some(444),
            propagation_stamp_cost: Some(7),
            propagation_stamp_cost_flexibility: Some(2),
            peering_cost: Some(9),
            peering_key_stamp: None,
            peering_key_value: None,
            restored_handled_ids: Vec::new(),
            restored_unhandled_ids: Vec::new(),
        };

        let value = serde_json::to_value(&record).expect("serialize peer record");
        assert_eq!(value["propagation_sync_limit"].as_u64(), Some(1));
        assert_eq!(value["sync_limit"].as_u64(), Some(444));

        let roundtrip: PeerRecord =
            serde_json::from_value(value).expect("roundtrip serialized peer record");
        assert_eq!(roundtrip.propagation_sync_limit, record.propagation_sync_limit);
    }

    #[test]
    fn peer_record_serialized_status_aliases_roundtrip() {
        let record = PeerRecord {
            peer: "peer-roundtrip-status".to_string(),
            last_seen: 1_700_001_006,
            capabilities: vec!["propagation".to_string(), "delivery".to_string()],
            name: Some("Peer Roundtrip Status".to_string()),
            name_source: Some("announce".to_string()),
            metadata: json!({"operator": "roundtrip"}),
            peer_type: Some("static".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_001,
            next_sync_attempt: 1_700_001_721,
            sync_backoff: 720,
            network_distance: 2,
            offered: 9,
            outgoing: 6,
            incoming: 4,
            rx_bytes: 12,
            tx_bytes: 34,
            sync_transfer_rate: 1024.0,
            acceptance_rate: 0.75,
            first_seen: 1_700_000_901,
            seen_count: 5,
            peering_timebase: 1_700_000_951,
            sync_strategy: 2,
            propagation_transfer_limit: Some(555),
            propagation_sync_limit: Some(666),
            propagation_stamp_cost: Some(8),
            propagation_stamp_cost_flexibility: Some(3),
            peering_cost: Some(10),
            peering_key_stamp: Some(b"roundtrip-stamp".to_vec()),
            peering_key_value: Some(10),
            restored_handled_ids: vec!["dd".repeat(32)],
            restored_unhandled_ids: vec!["ee".repeat(32), "ff".repeat(32)],
        };

        let value = serde_json::to_value(&record).expect("serialize peer record");
        let roundtrip: PeerRecord =
            serde_json::from_value(value).expect("deserialize serialized peer record");

        assert_eq!(roundtrip, record);
    }
}
