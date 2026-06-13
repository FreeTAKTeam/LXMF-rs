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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteTransferResult {
    pub remote: String,
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
    pub result: JsonValue,
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
