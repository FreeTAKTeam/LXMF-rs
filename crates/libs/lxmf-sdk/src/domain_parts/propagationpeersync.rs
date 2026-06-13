#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationPeerSyncRequest {
    pub peer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_limit_kb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wanted_ids: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub maintenance_claimed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub force_sync: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationPeerSyncResult {
    pub peer: String,
    #[serde(default)]
    pub peer_type: Option<String>,
    #[serde(default, alias = "type")]
    pub status_type: Option<String>,
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub postponed: bool,
    #[serde(default)]
    pub postpone_reason: Option<String>,
    #[serde(default)]
    pub last_sync_attempt: Option<i64>,
    #[serde(default)]
    pub next_sync_attempt: Option<i64>,
    #[serde(default)]
    pub sync_backoff: Option<u64>,
    #[serde(default)]
    pub transfer_limit: Option<u64>,
    #[serde(default)]
    pub sync_limit: Option<u64>,
    #[serde(default)]
    pub messages: JsonValue,
    #[serde(default)]
    pub propagation: JsonValue,
}

fn is_false(value: &bool) -> bool {
    !*value
}
