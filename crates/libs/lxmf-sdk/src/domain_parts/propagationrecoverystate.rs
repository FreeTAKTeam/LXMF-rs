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
        let state_name = json_string(&propagation, "state_name");
        let sync_state = json_u64(&propagation, "sync_state").unwrap_or(0) as u32;
        let failure_kind = json_string(&propagation, "failure_kind").or_else(|| match state_name.as_deref() {
            Some("no_access") => Some("no_access".to_string()),
            Some("timeout") => Some("timeout".to_string()),
            _ => None,
        });
        let timed_out = failure_kind.as_deref() == Some("timeout") || state_name.as_deref() == Some("timeout");
        let access_denied = json_bool(&propagation, "access_denied").unwrap_or(false)
            || state_name.as_deref() == Some("no_access")
            || sync_state == 0xf4
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        Self {
            enabled: json_bool(&propagation, "enabled").unwrap_or(false),
            selected_node: json_string(&propagation, "selected_node"),
            sync_state,
            state_name,
            sync_progress: json_f64(&propagation, "sync_progress"),
            last_sync_started: json_i64(&propagation, "last_sync_started"),
            last_sync_completed: json_i64(&propagation, "last_sync_completed"),
            last_sync_error: json_string(&propagation, "last_sync_error"),
            failure_kind,
            timed_out,
            access_denied,
            next_sync_attempt: json_i64(&propagation, "next_sync_attempt"),
            retry_count: json_u64(&propagation, "retry_count").unwrap_or(0),
            queue_depth: json_u64(&propagation, "queue_depth").unwrap_or(0),
            timestamp: json_i64(&propagation, "timestamp"),
            auth_required: json_bool(&propagation, "auth_required").unwrap_or(false),
            store_root: json_string(&propagation, "store_root"),
            target_cost: json_u64(&propagation, "target_cost"),
            stamp_cost_flexibility: json_u64(&propagation, "stamp_cost_flexibility"),
            message_storage_limit_mb: json_u64(&propagation, "message_storage_limit_mb"),
            delivery_limit: json_u64(&propagation, "delivery_limit"),
            propagation_limit: json_u64(&propagation, "propagation_limit"),
            autopeer: json_bool(&propagation, "autopeer"),
            autopeer_maxdepth: json_u64(&propagation, "autopeer_maxdepth"),
            static_peers: remote_transfer_json_string_array(&propagation, "static_peers"),
            sync_limit: json_u64(&propagation, "sync_limit"),
            max_peers: json_u64(&propagation, "max_peers"),
            from_static_only: json_bool(&propagation, "from_static_only"),
            retain_synced_on_node: json_bool(&propagation, "retain_synced_on_node"),
            peering_cost: json_u64(&propagation, "peering_cost"),
            remote_peering_cost_max: json_u64(&propagation, "remote_peering_cost_max"),
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
