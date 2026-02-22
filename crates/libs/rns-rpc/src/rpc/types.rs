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

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct RpcError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_user_actionable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<JsonMap<String, JsonValue>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Box<JsonMap<String, JsonValue>>>,
}

impl RpcError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        let message = message.into();
        let category = Self::category_for_code(code.as_str());
        let retryable = category
            .as_deref()
            .is_some_and(|value| value == "Transport" || value == "Timeout");
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct InterfaceRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct DeliveryPolicy {
    pub auth_required: bool,
    pub allowed_destinations: Vec<String>,
    pub denied_destinations: Vec<String>,
    pub ignored_destinations: Vec<String>,
    pub prioritised_destinations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct PropagationState {
    pub enabled: bool,
    pub store_root: Option<String>,
    pub target_cost: u32,
    pub total_ingested: usize,
    pub last_ingest_count: usize,
    pub sync_state: u32,
    pub state_name: String,
    pub sync_progress: f64,
    pub messages_received: usize,
    pub max_messages: usize,
    pub selected_node: Option<String>,
    pub last_sync_started: Option<i64>,
    pub last_sync_completed: Option<i64>,
    pub last_sync_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct StampPolicy {
    pub target_cost: u32,
    pub flexibility: u32,
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
    http_requests_by_route: BTreeMap<String, u64>,
    rpc_requests_by_method: BTreeMap<String, u64>,
    rpc_errors_by_method: BTreeMap<String, u64>,
    sdk_event_sink_publish_by_kind: BTreeMap<String, u64>,
    sdk_event_sink_errors_by_kind: BTreeMap<String, u64>,
    sdk_send_latency_ms: RpcLatencyHistogram,
    sdk_poll_latency_ms: RpcLatencyHistogram,
    sdk_auth_latency_ms: RpcLatencyHistogram,
}

pub struct RpcDaemon {
    store: MessagesStore,
    identity_hash: String,
    delivery_destination_hash: Mutex<Option<String>>,
    events: broadcast::Sender<RpcEvent>,
    event_queue: Mutex<VecDeque<RpcEvent>>,
    sdk_event_log: Mutex<VecDeque<SequencedRpcEvent>>,
    sdk_next_event_seq: Mutex<u64>,
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
    sdk_markers: Mutex<HashMap<String, SdkMarkerRecord>>,
    sdk_marker_order: Mutex<Vec<String>>,
    sdk_identities: Mutex<HashMap<String, SdkIdentityBundle>>,
    sdk_contacts: Mutex<HashMap<String, SdkContactRecord>>,
    sdk_contact_order: Mutex<Vec<String>>,
    sdk_active_identity: Mutex<Option<String>>,
    sdk_remote_commands: Mutex<HashSet<String>>,
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
    delivery_traces: Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>,
    delivery_status_lock: Mutex<()>,
    sdk_metrics: Mutex<RpcMetrics>,
    outbound_bridge: Option<Arc<dyn OutboundBridge>>,
    announce_bridge: Option<Arc<dyn AnnounceBridge>>,
    event_sink_bridges: Vec<Arc<dyn EventSinkBridge>>,
}

pub trait OutboundBridge: Send + Sync {
    fn deliver(
        &self,
        record: &MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error>;
}

pub trait AnnounceBridge: Send + Sync {
    fn announce_now(&self) -> Result<(), std::io::Error>;
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RpcEvent {
    pub event_type: String,
    pub payload: JsonValue,
}

#[derive(Debug, Clone)]
struct SequencedRpcEvent {
    seq_no: u64,
    event: RpcEvent,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PeerRecord {
    pub peer: String,
    pub last_seen: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_source: Option<String>,
    #[serde(default)]
    pub first_seen: i64,
    #[serde(default)]
    pub seen_count: u64,
}
