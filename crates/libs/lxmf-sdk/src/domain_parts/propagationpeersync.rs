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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub struct PropagationPeerQueueSnapshot {
    #[serde(default)]
    pub offered: u64,
    #[serde(default)]
    pub outgoing: u64,
    #[serde(default)]
    pub incoming: u64,
    #[serde(default)]
    pub unhandled: u64,
    #[serde(default)]
    pub offered_bytes: u64,
    #[serde(default)]
    pub unhandled_bytes: u64,
    #[serde(default)]
    pub transferred: u64,
    #[serde(default)]
    pub skipped: u64,
    #[serde(default)]
    pub rejected: u64,
    #[serde(default)]
    pub transfer_limited: u64,
    #[serde(default)]
    pub transferred_bytes: u64,
    #[serde(default)]
    pub skipped_bytes: u64,
    #[serde(default)]
    pub rejected_bytes: u64,
    #[serde(default)]
    pub transfer_limited_bytes: u64,
    #[serde(default)]
    pub handled_ids: Vec<String>,
    #[serde(default)]
    pub unhandled_ids: Vec<String>,
    #[serde(default)]
    pub transferred_ids: Vec<String>,
    #[serde(default)]
    pub skipped_ids: Vec<String>,
    #[serde(default)]
    pub rejected_ids: Vec<String>,
    #[serde(default)]
    pub transfer_limited_ids: Vec<String>,
}

impl PropagationPeerQueueSnapshot {
    fn from_messages_and_propagation(messages: &JsonValue, propagation: &JsonValue) -> Self {
        Self {
            offered: peer_queue_json_u64(messages, "offered").ok().flatten().unwrap_or(0),
            outgoing: peer_queue_json_u64(messages, "outgoing").ok().flatten().unwrap_or(0),
            incoming: peer_queue_json_u64(messages, "incoming").ok().flatten().unwrap_or(0),
            unhandled: peer_queue_json_u64(messages, "unhandled").ok().flatten().unwrap_or(0),
            offered_bytes: peer_queue_json_u64(messages, "offered_bytes").ok().flatten().unwrap_or(0),
            unhandled_bytes: peer_queue_json_u64(messages, "unhandled_bytes").ok().flatten().unwrap_or(0),
            transferred: peer_queue_json_u64(propagation, "transferred").ok().flatten().unwrap_or(0),
            skipped: peer_queue_json_u64(propagation, "skipped").ok().flatten()
                .or_else(|| peer_queue_json_u64(propagation, "remaining").ok().flatten())
                .unwrap_or(0),
            rejected: peer_queue_json_u64(propagation, "rejected").ok().flatten().unwrap_or(0),
            transfer_limited: peer_queue_json_u64(propagation, "transfer_limited").ok().flatten().unwrap_or(0),
            transferred_bytes: peer_queue_json_u64(propagation, "bytes").ok().flatten().unwrap_or(0),
            skipped_bytes: peer_queue_json_u64(propagation, "skipped_bytes").ok().flatten()
                .or_else(|| peer_queue_json_u64(propagation, "remaining_bytes").ok().flatten())
                .unwrap_or(0),
            rejected_bytes: peer_queue_json_u64(propagation, "rejected_bytes").ok().flatten().unwrap_or(0),
            transfer_limited_bytes: peer_queue_json_u64(propagation, "transfer_limited_bytes")
                .ok().flatten().unwrap_or(0),
            handled_ids: peer_queue_json_string_array(messages, "handled_ids"),
            unhandled_ids: peer_queue_json_string_array(messages, "unhandled_ids"),
            transferred_ids: peer_queue_json_string_array(propagation, "transferred_ids"),
            skipped_ids: peer_queue_json_string_array(propagation, "skipped_ids"),
            rejected_ids: peer_queue_json_string_array(propagation, "rejected_ids"),
            transfer_limited_ids: peer_queue_json_string_array(propagation, "transfer_limited_ids"),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
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
    pub failure_kind: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub access_denied: bool,
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
    pub target_stamp_cost: Option<u64>,
    #[serde(default)]
    pub stamp_cost_flexibility: Option<u64>,
    #[serde(default)]
    pub messages: JsonValue,
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub queue: PropagationPeerQueueSnapshot,
}

#[derive(Deserialize)]
struct RawPropagationPeerSyncResult {
    peer: String,
    #[serde(default)]
    peer_type: Option<String>,
    #[serde(default, alias = "type")]
    status_type: Option<String>,
    #[serde(default)]
    synced: bool,
    #[serde(default)]
    postponed: bool,
    #[serde(default)]
    postpone_reason: Option<String>,
    #[serde(default)]
    failure_kind: Option<String>,
    #[serde(default)]
    access_denied: Option<bool>,
    #[serde(default)]
    last_sync_attempt: Option<i64>,
    #[serde(default)]
    next_sync_attempt: Option<i64>,
    #[serde(default)]
    sync_backoff: Option<u64>,
    #[serde(default)]
    transfer_limit: Option<u64>,
    #[serde(default)]
    sync_limit: Option<u64>,
    #[serde(default)]
    target_stamp_cost: Option<u64>,
    #[serde(default)]
    stamp_cost_flexibility: Option<u64>,
    #[serde(default)]
    messages: JsonValue,
    #[serde(default)]
    propagation: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationPeerSyncResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationPeerSyncResult::deserialize(deserializer)?;
        let queue =
            PropagationPeerQueueSnapshot::from_messages_and_propagation(&raw.messages, &raw.propagation);
        let postponed =
            raw.postponed || peer_queue_json_bool(&raw.propagation, "postponed").ok().flatten().unwrap_or(false);
        let postpone_reason = raw
            .postpone_reason
            .or_else(|| peer_queue_json_string(&raw.propagation, "postpone_reason").ok().flatten());
        let failure_kind = raw
            .failure_kind
            .or_else(|| peer_queue_json_string(&raw.propagation, "failure_kind").ok().flatten());
        let timed_out = failure_kind.as_deref() == Some("timeout")
            || postpone_reason.as_deref() == Some("timeout");
        let access_denied = raw.access_denied.unwrap_or(false)
            || peer_queue_json_bool(&raw.propagation, "access_denied").ok().flatten().unwrap_or(false)
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        let transfer_limit = raw
            .transfer_limit
            .or_else(|| peer_queue_json_u64(&raw.propagation, "transfer_limit").ok().flatten());
        let sync_limit = raw
            .sync_limit
            .or_else(|| peer_queue_json_u64(&raw.propagation, "sync_limit").ok().flatten());
        let target_stamp_cost = raw
            .target_stamp_cost
            .or_else(|| peer_queue_json_u64(&raw.propagation, "target_stamp_cost").ok().flatten());
        let stamp_cost_flexibility = raw
            .stamp_cost_flexibility
            .or_else(|| peer_queue_json_u64(&raw.propagation, "stamp_cost_flexibility").ok().flatten());
        Ok(Self {
            peer: raw.peer,
            peer_type: raw.peer_type,
            status_type: raw.status_type,
            synced: raw.synced,
            postponed,
            postpone_reason,
            failure_kind,
            timed_out,
            access_denied,
            last_sync_attempt: raw.last_sync_attempt,
            next_sync_attempt: raw.next_sync_attempt,
            sync_backoff: raw.sync_backoff,
            transfer_limit,
            sync_limit,
            target_stamp_cost,
            stamp_cost_flexibility,
            messages: raw.messages,
            propagation: raw.propagation,
            queue,
        })
    }
}

fn peer_queue_json_bool(value: &JsonValue, key: &str) -> Result<Option<bool>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_bool().ok_or("field is not a bool").map(Some),
    }
}

fn peer_queue_json_string(value: &JsonValue, key: &str) -> Result<Option<String>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_str().ok_or("field is not a string").map(|s| Some(s.to_owned())),
    }
}

fn peer_queue_json_u64(value: &JsonValue, key: &str) -> Result<Option<u64>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_u64().ok_or("field is not an unsigned integer").map(Some),
    }
}

fn peer_queue_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn is_false(value: &bool) -> bool {
    !*value
}
