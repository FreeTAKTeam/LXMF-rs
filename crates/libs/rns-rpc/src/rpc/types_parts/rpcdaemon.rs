pub struct RpcDaemon {
    store: Arc<MessagesStore>,
    identity_hash: String,
    delivery_destination_hash: Mutex<Option<String>>,
    propagation_destination_hash: Mutex<Option<String>>,
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
    sdk_identity_sessions: Mutex<HashMap<String, SdkIdentitySession>>,
    sdk_contacts: Mutex<HashMap<String, SdkContactRecord>>,
    sdk_contact_order: Mutex<Vec<String>>,
    sdk_active_identity: Mutex<Option<String>>,
    sdk_remote_commands: Mutex<HashMap<String, SdkRemoteCommandRecord>>,
    sdk_voice_sessions: Mutex<HashMap<String, SdkVoiceSessionRecord>>,
    peers: Mutex<HashMap<String, PeerRecord>>,
    interfaces: Mutex<Vec<InterfaceRecord>>,
    delivery_policy: Mutex<DeliveryPolicy>,
    blackholed_identities: Mutex<HashMap<String, JsonValue>>,
    propagation_state: Mutex<PropagationState>,
    remote_unpeer_failure_state: Mutex<Option<PropagationState>>,
    propagation_payloads: Mutex<HashMap<String, String>>,
    throttled_propagation_peers: Mutex<HashMap<String, i64>>,
    outbound_propagation_node: Mutex<Option<String>>,
    paper_ingest_seen: Mutex<HashSet<String>>,
    stamp_policy: Mutex<StampPolicy>,
    ticket_cache: Mutex<HashMap<String, TicketRecord>>,
    ticket_last_deliveries: Mutex<HashMap<String, i64>>,
    router_information_storage_limit_bytes: Mutex<Option<u64>>,
    router_retain_node_lxms: Mutex<bool>,
    delivery_traces: Arc<Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>>,
    daemon_status_snapshot: std::sync::RwLock<DaemonStatusSnapshot>,
    delivery_status_lock: Arc<Mutex<()>>,
    outbound_delivery_handoffs: Arc<Mutex<HashSet<String>>>,
    sdk_metrics: Arc<Mutex<RpcMetrics>>,
    outbound_bridge: Option<Arc<dyn OutboundBridge>>,
    outbound_delivery_tx: Option<mpsc::SyncSender<OutboundDeliveryCommand>>,
    announce_bridge: Option<Arc<dyn AnnounceBridge>>,
    service_identity_bridge: Mutex<Option<Arc<dyn ServiceIdentityBridge>>>,
    event_sink_bridges: Vec<Arc<dyn EventSinkBridge>>,
    event_sink_tx: Option<mpsc::SyncSender<EventSinkCommand>>,
    interface_mutation_bridge: Mutex<Option<Arc<dyn InterfaceMutationBridge>>>,
    path_lookup_bridge: Mutex<Option<Arc<dyn PathLookupBridge>>>,
    remote_control_bridge: Mutex<Option<Arc<dyn RemoteControlBridge>>>,
    rnode_management_bridge: Mutex<Option<Arc<dyn RNodeManagementBridge>>>,
    weave_display_control_bridge: Mutex<Option<Arc<dyn WeaveDisplayControlBridge>>>,
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

    fn announce_delivery(&self, _destination_hash: &str) -> Result<(), std::io::Error> {
        Err(std::io::Error::other("targeted delivery announce is not configured"))
    }
}

pub trait InterfaceMutationBridge: Send + Sync {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, std::io::Error>;
}

include!("path_lookup_bridge.rs");

pub trait RNodeManagementBridge: Send + Sync {
    fn dispatch_rnode_management(
        &self,
        iface: &str,
        command: &str,
        params: &JsonValue,
    ) -> Result<JsonValue, std::io::Error>;
}

pub trait WeaveDisplayControlBridge: Send + Sync {
    fn set_weave_remote_display(
        &self,
        iface: &str,
        enable: bool,
        remote_switch_id_hex: Option<&str>,
    ) -> Result<JsonValue, std::io::Error>;
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
    pub sync_schedule_reason: Option<String>,
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
        if let Some(reason) = self.sync_schedule_reason.as_deref() {
            map.serialize_entry("sync_schedule_reason", reason)?;
        }
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
    #[serde(default)]
    sync_schedule_reason: Option<String>,
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
