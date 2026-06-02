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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PeerRecord {
    pub peer: String,
    pub last_seen: i64,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_source: Option<String>,
    #[serde(default)]
    pub peer_type: Option<String>,
    #[serde(default)]
    pub alive: bool,
    #[serde(default)]
    pub last_sync_attempt: i64,
    #[serde(default)]
    pub next_sync_attempt: i64,
    #[serde(default)]
    pub sync_backoff: u32,
    #[serde(default = "default_network_distance")]
    pub network_distance: u32,
    #[serde(default)]
    pub rx_bytes: u64,
    #[serde(default)]
    pub tx_bytes: u64,
    #[serde(default = "default_acceptance_rate")]
    pub acceptance_rate: f64,
    #[serde(default)]
    pub first_seen: i64,
    #[serde(default)]
    pub seen_count: u64,
    #[serde(default)]
    pub peering_timebase: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation_transfer_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation_sync_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation_stamp_cost: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation_stamp_cost_flexibility: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peering_cost: Option<u32>,
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

fn default_acceptance_rate() -> f64 {
    0.0
}
