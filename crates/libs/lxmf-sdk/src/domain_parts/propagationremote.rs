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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_allowed: Option<Vec<String>>,
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
pub struct PropagationDeliveryPolicyEntryRequest {
    pub destination: String,
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

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteStatusResult {
    pub remote: String,
    #[serde(default)]
    pub status: JsonValue,
    #[serde(default)]
    pub status_state: PropagationRemoteStatusState,
    #[serde(default)]
    pub stats: PropagationRemoteStats,
}

#[derive(Deserialize)]
struct RawPropagationRemoteStatusResult {
    remote: String,
    #[serde(default)]
    status: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationRemoteStatusResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationRemoteStatusResult::deserialize(deserializer)?;
        let status_state = PropagationRemoteStatusState::from_status(&raw.status);
        let stats = PropagationRemoteStats::from_status(&raw.status);
        Ok(Self {
            remote: raw.remote,
            status: raw.status,
            status_state,
            stats,
        })
    }
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
    pub selected_node: Option<String>,
    #[serde(default)]
    pub selected_peer: Option<String>,
    #[serde(default)]
    pub sync_progress: Option<f64>,
    #[serde(default)]
    pub last_sync_started: Option<i64>,
    #[serde(default)]
    pub last_sync_completed: Option<i64>,
    #[serde(default)]
    pub last_sync_error: Option<String>,
    #[serde(default)]
    pub failure_kind: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub access_denied: bool,
    #[serde(default)]
    pub retry_count: u64,
    #[serde(default)]
    pub next_sync_attempt: Option<i64>,
}

impl PropagationRemoteTransferState {
    pub(crate) fn from_result_and_propagation(result: &JsonValue, propagation: &JsonValue) -> Self {
        let state_name = json_string(propagation, "state_name").ok().flatten();
        let sync_state = json_u64(propagation, "sync_state").ok().flatten().unwrap_or(0) as u32;
        let failure_kind = json_string(result, "failure_kind").ok().flatten()
            .or_else(|| json_string(propagation, "failure_kind").ok().flatten())
            .or_else(|| match state_name.as_deref() {
                Some("no_access") => Some("no_access".to_string()),
                Some("timeout") => Some("timeout".to_string()),
                _ => None,
            });
        let timed_out = failure_kind.as_deref() == Some("timeout")
            || json_string(result, "postpone_reason").ok().flatten().as_deref() == Some("timeout")
            || state_name.as_deref() == Some("timeout");
        let access_denied = json_bool(propagation, "access_denied").ok().flatten().unwrap_or(false)
            || state_name.as_deref() == Some("no_access")
            || sync_state == 0xf4
            || matches!(failure_kind.as_deref(), Some("access_denied" | "access-denied" | "no_access"));
        Self {
            synced: json_bool(result, "synced").ok().flatten().unwrap_or(false),
            postponed: json_bool(result, "postponed").ok().flatten().unwrap_or(false),
            postpone_reason: json_string(result, "postpone_reason").ok().flatten(),
            imported_count: json_u64(result, "imported_count").ok().flatten().unwrap_or(0),
            imported_ids: remote_transfer_json_string_array(result, "imported_ids"),
            transferred_bytes: json_u64(result, "transferred_bytes").ok().flatten().unwrap_or(0),
            state_name,
            selected_node: json_string(propagation, "selected_node").ok().flatten(),
            selected_peer: json_string(propagation, "selected_peer").ok().flatten(),
            sync_progress: json_f64(propagation, "sync_progress").ok().flatten(),
            last_sync_started: json_i64(propagation, "last_sync_started").ok().flatten(),
            last_sync_completed: json_i64(propagation, "last_sync_completed").ok().flatten(),
            last_sync_error: json_string(propagation, "last_sync_error").ok().flatten(),
            failure_kind,
            timed_out,
            access_denied,
            retry_count: json_u64(propagation, "retry_count").ok().flatten().unwrap_or(0),
            next_sync_attempt: json_i64(propagation, "next_sync_attempt").ok().flatten(),
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
    #[serde(default)]
    pub queue: PropagationPeerQueueSnapshot,
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
        let queue =
            PropagationPeerQueueSnapshot::from_messages_and_propagation(&JsonValue::Null, &raw.propagation);
        let transfer_state =
            PropagationRemoteTransferState::from_result_and_propagation(&raw.result, &raw.propagation);
        Ok(Self {
            remote: raw.remote,
            propagation: raw.propagation,
            result: raw.result,
            transfer_state,
            queue,
        })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationAcknowledgeSyncResult {
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub recovery_state: PropagationRecoveryStateResult,
}

#[derive(Deserialize)]
struct RawPropagationAcknowledgeSyncResult {
    #[serde(default)]
    propagation: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationAcknowledgeSyncResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationAcknowledgeSyncResult::deserialize(deserializer)?;
        let recovery_state = PropagationRecoveryStateResult::try_from_propagation(
            raw.propagation.clone(),
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            propagation: raw.propagation,
            recovery_state,
        })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationStatusResult {
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub recovery_state: PropagationRecoveryStateResult,
}

#[derive(Deserialize)]
struct RawPropagationStatusResult {
    #[serde(default)]
    propagation: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationStatusResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationStatusResult::deserialize(deserializer)?;
        let recovery_state = PropagationRecoveryStateResult::try_from_propagation(
            raw.propagation.clone(),
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            propagation: raw.propagation,
            recovery_state,
        })
    }
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
    pub transfer_state: PropagationRemoteTransferState,
    #[serde(default)]
    pub queue: PropagationPeerQueueSnapshot,
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
        let transfer_state =
            PropagationRemoteTransferState::from_result_and_propagation(&raw.result, &raw.propagation);
        let empty_messages = JsonValue::Null;
        let queue_messages = raw.peer_sync.get("messages").unwrap_or(&empty_messages);
        let queue = PropagationPeerQueueSnapshot::from_messages_and_propagation(queue_messages, &raw.propagation);
        Ok(Self {
            remote: raw.remote,
            peer: raw.peer,
            propagation: raw.propagation,
            peer_sync: raw.peer_sync,
            peer_sync_state,
            transfer_state,
            queue,
            result: raw.result,
        })
    }
}
