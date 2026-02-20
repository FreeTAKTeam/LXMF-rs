#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcResponse {
    pub id: u64,
    pub result: Option<JsonValue>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: String,
    pub message: String,
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
    sdk_next_domain_seq: Mutex<u64>,
    sdk_topics: Mutex<HashMap<String, SdkTopicRecord>>,
    sdk_topic_order: Mutex<Vec<String>>,
    sdk_topic_subscriptions: Mutex<HashSet<String>>,
    sdk_telemetry_points: Mutex<Vec<SdkTelemetryPoint>>,
    sdk_attachments: Mutex<HashMap<String, SdkAttachmentRecord>>,
    sdk_attachment_payloads: Mutex<HashMap<String, String>>,
    sdk_attachment_order: Mutex<Vec<String>>,
    sdk_markers: Mutex<HashMap<String, SdkMarkerRecord>>,
    sdk_marker_order: Mutex<Vec<String>>,
    sdk_identities: Mutex<HashMap<String, SdkIdentityBundle>>,
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
    outbound_bridge: Option<Arc<dyn OutboundBridge>>,
    announce_bridge: Option<Arc<dyn AnnounceBridge>>,
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
