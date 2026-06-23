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
    pub failure_kind: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub access_denied: bool,
    #[serde(default)]
    pub next_sync_attempt: Option<i64>,
    #[serde(default)]
    pub retry_count: u64,
    #[serde(default)]
    pub queue_depth: u64,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub auth_required: bool,
    #[serde(default)]
    pub store_root: Option<String>,
    #[serde(default)]
    pub target_cost: Option<u64>,
    #[serde(default)]
    pub stamp_cost_flexibility: Option<u64>,
    #[serde(default)]
    pub message_storage_limit_mb: Option<u64>,
    #[serde(default)]
    pub delivery_limit: Option<u64>,
    #[serde(default)]
    pub propagation_limit: Option<u64>,
    #[serde(default)]
    pub autopeer: Option<bool>,
    #[serde(default)]
    pub autopeer_maxdepth: Option<u64>,
    #[serde(default)]
    pub static_peers: Vec<String>,
    #[serde(default)]
    pub sync_limit: Option<u64>,
    #[serde(default)]
    pub max_peers: Option<u64>,
    #[serde(default)]
    pub from_static_only: Option<bool>,
    #[serde(default)]
    pub retain_synced_on_node: Option<bool>,
    #[serde(default)]
    pub peering_cost: Option<u64>,
    #[serde(default)]
    pub remote_peering_cost_max: Option<u64>,
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
        let state_name = json_string(&propagation, "state_name").ok().flatten();
        let sync_state = json_u64(&propagation, "sync_state").ok().flatten().unwrap_or(0) as u32;
        let failure_kind = json_string(&propagation, "failure_kind").ok().flatten().or_else(|| match state_name.as_deref() {
            Some("no_access") => Some("no_access".to_string()),
            Some("timeout") => Some("timeout".to_string()),
            _ => None,
        });
        let timed_out = failure_kind.as_deref() == Some("timeout") || state_name.as_deref() == Some("timeout");
        let access_denied = json_bool(&propagation, "access_denied").ok().flatten().unwrap_or(false)
            || state_name.as_deref() == Some("no_access")
            || sync_state == 0xf4
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        Self {
            enabled: json_bool(&propagation, "enabled").ok().flatten().unwrap_or(false),
            selected_node: json_string(&propagation, "selected_node").ok().flatten(),
            sync_state,
            state_name,
            sync_progress: json_f64(&propagation, "sync_progress").ok().flatten(),
            last_sync_started: json_i64(&propagation, "last_sync_started").ok().flatten(),
            last_sync_completed: json_i64(&propagation, "last_sync_completed").ok().flatten(),
            last_sync_error: json_string(&propagation, "last_sync_error").ok().flatten(),
            failure_kind,
            timed_out,
            access_denied,
            next_sync_attempt: json_i64(&propagation, "next_sync_attempt").ok().flatten(),
            retry_count: json_u64(&propagation, "retry_count").ok().flatten().unwrap_or(0),
            queue_depth: json_u64(&propagation, "queue_depth").ok().flatten().unwrap_or(0),
            timestamp: json_i64(&propagation, "timestamp").ok().flatten(),
            auth_required: json_bool(&propagation, "auth_required").ok().flatten().unwrap_or(false),
            store_root: json_string(&propagation, "store_root").ok().flatten(),
            target_cost: json_u64(&propagation, "target_cost").ok().flatten(),
            stamp_cost_flexibility: json_u64(&propagation, "stamp_cost_flexibility").ok().flatten(),
            message_storage_limit_mb: json_u64(&propagation, "message_storage_limit_mb").ok().flatten(),
            delivery_limit: json_u64(&propagation, "delivery_limit").ok().flatten(),
            propagation_limit: json_u64(&propagation, "propagation_limit").ok().flatten(),
            autopeer: json_bool(&propagation, "autopeer").ok().flatten(),
            autopeer_maxdepth: json_u64(&propagation, "autopeer_maxdepth").ok().flatten(),
            static_peers: remote_transfer_json_string_array(&propagation, "static_peers"),
            sync_limit: json_u64(&propagation, "sync_limit").ok().flatten(),
            max_peers: json_u64(&propagation, "max_peers").ok().flatten(),
            from_static_only: json_bool(&propagation, "from_static_only").ok().flatten(),
            retain_synced_on_node: json_bool(&propagation, "retain_synced_on_node").ok().flatten(),
            peering_cost: json_u64(&propagation, "peering_cost").ok().flatten(),
            remote_peering_cost_max: json_u64(&propagation, "remote_peering_cost_max").ok().flatten(),
            total_ingested: json_u64(&propagation, "total_ingested").ok().flatten().unwrap_or(0),
            last_ingest_count: json_u64(&propagation, "last_ingest_count").ok().flatten().unwrap_or(0),
            client_propagation_messages_received: json_u64(
                &propagation,
                "client_propagation_messages_received",
            )
            .ok().flatten().unwrap_or(0),
            client_propagation_messages_served: json_u64(
                &propagation,
                "client_propagation_messages_served",
            )
            .ok().flatten().unwrap_or(0),
            propagation,
        }
    }
}

fn json_bool(value: &JsonValue, key: &str) -> Result<Option<bool>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_bool().ok_or("field is not a bool").map(Some),
    }
}

fn json_f64(value: &JsonValue, key: &str) -> Result<Option<f64>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_f64().ok_or("field is not a number").map(Some),
    }
}

fn json_i64(value: &JsonValue, key: &str) -> Result<Option<i64>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_i64().ok_or("field is not an integer").map(Some),
    }
}

fn json_u64(value: &JsonValue, key: &str) -> Result<Option<u64>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_u64().ok_or("field is not an unsigned integer").map(Some),
    }
}

fn json_string(value: &JsonValue, key: &str) -> Result<Option<String>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_str().ok_or("field is not a string").map(|s| Some(s.to_owned())),
    }
}

fn remote_transfer_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}
