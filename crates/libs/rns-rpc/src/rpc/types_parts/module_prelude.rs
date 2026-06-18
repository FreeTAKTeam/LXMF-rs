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
    #[serde(default)]
    pub propagation_node_enabled: bool,
    #[serde(default)]
    pub auth_required: bool,
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
    #[serde(default)]
    pub retain_synced_on_node: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peering_cost: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_peering_cost_max: Option<u32>,
    #[serde(default)]
    pub peer_announce_at_start: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_announce_interval_secs: Option<u64>,
    #[serde(default)]
    pub node_announce_at_start: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_announce_interval_secs: Option<u64>,
    #[serde(default)]
    pub control_allowed: Vec<String>,
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
            propagation_node_enabled: false,
            auth_required: false,
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
            retain_synced_on_node: false,
            peering_cost: None,
            remote_peering_cost_max: None,
            peer_announce_at_start: false,
            peer_announce_interval_secs: None,
            node_announce_at_start: false,
            node_announce_interval_secs: None,
            control_allowed: Vec::new(),
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
