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

#[derive(Debug, Deserialize)]
struct RecordReceiptParams {
    message_id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct ReceiveMessageParams {
    id: String,
    source: String,
    destination: String,
    #[serde(default)]
    title: String,
    content: String,
    fields: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct AnnounceReceivedParams {
    peer: String,
    timestamp: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    name_source: Option<String>,
    #[serde(default)]
    app_data_hex: Option<String>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    rssi: Option<f64>,
    #[serde(default)]
    snr: Option<f64>,
    #[serde(default)]
    q: Option<f64>,
    #[serde(default)]
    stamp_cost_flexibility: Option<u32>,
    #[serde(default)]
    peering_cost: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SetInterfacesParams {
    interfaces: Vec<InterfaceRecord>,
}

#[derive(Debug, Deserialize)]
struct PeerOpParams {
    peer: String,
}

#[derive(Debug, Deserialize)]
struct DeliveryPolicyParams {
    #[serde(default)]
    auth_required: Option<bool>,
    #[serde(default)]
    allowed_destinations: Option<Vec<String>>,
    #[serde(default)]
    denied_destinations: Option<Vec<String>>,
    #[serde(default)]
    ignored_destinations: Option<Vec<String>>,
    #[serde(default)]
    prioritised_destinations: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct PropagationEnableParams {
    enabled: bool,
    #[serde(default)]
    store_root: Option<String>,
    #[serde(default)]
    target_cost: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PropagationIngestParams {
    #[serde(default)]
    transient_id: Option<String>,
    #[serde(default)]
    payload_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PropagationFetchParams {
    transient_id: String,
}

#[derive(Debug, Deserialize)]
struct PaperIngestUriParams {
    uri: String,
}

#[derive(Debug, Deserialize)]
struct StampPolicySetParams {
    #[serde(default)]
    target_cost: Option<u32>,
    #[serde(default)]
    flexibility: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TicketGenerateParams {
    destination: String,
    #[serde(default)]
    ttl_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct ListAnnouncesParams {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_ts: Option<i64>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetOutboundPropagationNodeParams {
    #[serde(default)]
    peer: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageDeliveryTraceParams {
    message_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct PropagationNodeRecord {
    peer: String,
    #[serde(default)]
    name: Option<String>,
    last_seen: i64,
    #[serde(default)]
    capabilities: Vec<String>,
    selected: bool,
}
