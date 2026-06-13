#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteRequest {
    pub remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_private_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_limit_kb: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemotePeerRequest {
    pub remote: String,
    pub peer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_private_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_limit_kb: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationAcknowledgeSyncRequest {
    #[serde(default, skip_serializing_if = "is_false")]
    pub reset_state: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_state: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationNodeSetRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationEnableRequest {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_cost: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp_cost_flexibility: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_storage_limit_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propagation_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopeer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopeer_maxdepth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_peers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_peers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_static_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retain_synced_on_node: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peering_cost: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_peering_cost_max: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationDeliveryPolicyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_destinations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denied_destinations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_destinations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prioritised_destinations: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationIngestRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transient_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_hex: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationFetchRequest {
    pub transient_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteStatusResult {
    pub remote: String,
    #[serde(default)]
    pub status: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub struct PropagationRemoteTransferState {
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub postponed: bool,
    #[serde(default)]
    pub postpone_reason: Option<String>,
    #[serde(default)]
    pub imported_count: u64,
    #[serde(default)]
    pub imported_ids: Vec<String>,
    #[serde(default)]
    pub transferred_bytes: u64,
    #[serde(default)]
    pub state_name: Option<String>,
    #[serde(default)]
    pub sync_progress: Option<f64>,
    #[serde(default)]
    pub last_sync_error: Option<String>,
}

impl PropagationRemoteTransferState {
    fn from_result_and_propagation(result: &JsonValue, propagation: &JsonValue) -> Self {
        Self {
            synced: json_bool(result, "synced").unwrap_or(false),
            postponed: json_bool(result, "postponed").unwrap_or(false),
            postpone_reason: json_string(result, "postpone_reason"),
            imported_count: json_u64(result, "imported_count").unwrap_or(0),
            imported_ids: remote_transfer_json_string_array(result, "imported_ids"),
            transferred_bytes: json_u64(result, "transferred_bytes").unwrap_or(0),
            state_name: json_string(propagation, "state_name"),
            sync_progress: json_f64(propagation, "sync_progress"),
            last_sync_error: json_string(propagation, "last_sync_error"),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteTransferResult {
    pub remote: String,
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
    #[serde(default)]
    pub transfer_state: PropagationRemoteTransferState,
}

#[derive(Deserialize)]
struct RawPropagationRemoteTransferResult {
    remote: String,
    #[serde(default)]
    propagation: JsonValue,
    #[serde(default)]
    result: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationRemoteTransferResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationRemoteTransferResult::deserialize(deserializer)?;
        let transfer_state =
            PropagationRemoteTransferState::from_result_and_propagation(&raw.result, &raw.propagation);
        Ok(Self {
            remote: raw.remote,
            propagation: raw.propagation,
            result: raw.result,
            transfer_state,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationAcknowledgeSyncResult {
    #[serde(default)]
    pub propagation: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationNodeSelectionResult {
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub meta: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationNodeListResult {
    #[serde(default)]
    pub nodes: Vec<JsonValue>,
    #[serde(default)]
    pub meta: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationStatusResult {
    #[serde(default)]
    pub propagation: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRecoveryStateResult {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub selected_node: Option<String>,
    #[serde(default)]
    pub sync_state: u32,
    #[serde(default)]
    pub state_name: Option<String>,
    #[serde(default)]
    pub sync_progress: Option<f64>,
    #[serde(default)]
    pub last_sync_started: Option<i64>,
    #[serde(default)]
    pub last_sync_completed: Option<i64>,
    #[serde(default)]
    pub last_sync_error: Option<String>,
    #[serde(default)]
    pub retry_count: u64,
    #[serde(default)]
    pub queue_depth: u64,
    #[serde(default)]
    pub total_ingested: u64,
    #[serde(default)]
    pub last_ingest_count: u64,
    #[serde(default)]
    pub client_propagation_messages_received: u64,
    #[serde(default)]
    pub client_propagation_messages_served: u64,
    #[serde(default)]
    pub propagation: JsonValue,
}

impl PropagationRecoveryStateResult {
    pub fn from_propagation(propagation: JsonValue) -> Self {
        Self {
            enabled: json_bool(&propagation, "enabled").unwrap_or(false),
            selected_node: json_string(&propagation, "selected_node"),
            sync_state: json_u64(&propagation, "sync_state").unwrap_or(0) as u32,
            state_name: json_string(&propagation, "state_name"),
            sync_progress: json_f64(&propagation, "sync_progress"),
            last_sync_started: json_i64(&propagation, "last_sync_started"),
            last_sync_completed: json_i64(&propagation, "last_sync_completed"),
            last_sync_error: json_string(&propagation, "last_sync_error"),
            retry_count: json_u64(&propagation, "retry_count").unwrap_or(0),
            queue_depth: json_u64(&propagation, "queue_depth").unwrap_or(0),
            total_ingested: json_u64(&propagation, "total_ingested").unwrap_or(0),
            last_ingest_count: json_u64(&propagation, "last_ingest_count").unwrap_or(0),
            client_propagation_messages_received: json_u64(
                &propagation,
                "client_propagation_messages_received",
            )
            .unwrap_or(0),
            client_propagation_messages_served: json_u64(
                &propagation,
                "client_propagation_messages_served",
            )
            .unwrap_or(0),
            propagation,
        }
    }
}

fn json_bool(value: &JsonValue, key: &str) -> Option<bool> {
    value.get(key).and_then(JsonValue::as_bool)
}

fn json_f64(value: &JsonValue, key: &str) -> Option<f64> {
    value.get(key).and_then(JsonValue::as_f64)
}

fn json_i64(value: &JsonValue, key: &str) -> Option<i64> {
    value.get(key).and_then(JsonValue::as_i64)
}

fn json_u64(value: &JsonValue, key: &str) -> Option<u64> {
    value.get(key).and_then(JsonValue::as_u64)
}

fn json_string(value: &JsonValue, key: &str) -> Option<String> {
    value.get(key).and_then(JsonValue::as_str).map(ToOwned::to_owned)
}

fn remote_transfer_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationDeliveryPolicyResult {
    #[serde(default)]
    pub policy: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationPeerMaintenanceResult {
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub culled: u64,
    #[serde(default)]
    pub culled_peers: Vec<String>,
    #[serde(default)]
    pub rotated: u64,
    #[serde(default)]
    pub rotated_peers: Vec<String>,
    #[serde(default)]
    pub synced_peer: Option<String>,
    #[serde(default)]
    pub peer_sync: JsonValue,
    #[serde(default)]
    pub max_unreachable_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationIngestResult {
    #[serde(default)]
    pub ingested_count: u64,
    #[serde(default)]
    pub duplicate_count: u64,
    #[serde(default)]
    pub payload_bytes: u64,
    #[serde(default)]
    pub transferred_bytes: u64,
    #[serde(default)]
    pub transient_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationFetchResult {
    #[serde(default)]
    pub transient_id: String,
    #[serde(default)]
    pub payload_hex: String,
    #[serde(default)]
    pub payload_bytes: u64,
    #[serde(default)]
    pub transferred_bytes: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteSyncResult {
    pub remote: String,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub peer_sync: JsonValue,
    #[serde(default)]
    pub peer_sync_state: Option<PropagationPeerSyncResult>,
    #[serde(default)]
    pub result: JsonValue,
}

#[derive(Deserialize)]
struct RawPropagationRemoteSyncResult {
    remote: String,
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    propagation: JsonValue,
    #[serde(default)]
    peer_sync: JsonValue,
    #[serde(default)]
    result: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationRemoteSyncResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationRemoteSyncResult::deserialize(deserializer)?;
        let peer_sync_state = if raw.peer_sync.get("peer").is_some() {
            Some(
                serde_json::from_value::<PropagationPeerSyncResult>(raw.peer_sync.clone())
                    .map_err(serde::de::Error::custom)?,
            )
        } else {
            None
        };
        Ok(Self {
            remote: raw.remote,
            peer: raw.peer,
            propagation: raw.propagation,
            peer_sync: raw.peer_sync,
            peer_sync_state,
            result: raw.result,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteUnpeerResult {
    pub remote: String,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub propagation_cleared: Option<u64>,
    #[serde(default)]
    pub propagation_cleared_bytes: Option<u64>,
    #[serde(default)]
    pub messages: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
}
