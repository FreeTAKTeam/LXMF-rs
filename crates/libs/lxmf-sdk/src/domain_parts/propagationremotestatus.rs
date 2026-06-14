#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub struct PropagationRemoteStatusState {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub selected_node: Option<String>,
    #[serde(default)]
    pub selected_peer: Option<String>,
    #[serde(default)]
    pub failure_kind: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub access_denied: bool,
    #[serde(default)]
    pub queue_depth: u64,
    #[serde(default)]
    pub retry_count: u64,
    #[serde(default)]
    pub next_sync_attempt: Option<i64>,
    #[serde(default)]
    pub last_sync_error: Option<String>,
}

impl PropagationRemoteStatusState {
    fn from_status(status: &JsonValue) -> Self {
        let state = remote_status_json_string(status, "state")
            .or_else(|| remote_status_json_string(status, "state_name"));
        let failure_kind = remote_status_json_string(status, "failure_kind");
        let timed_out = failure_kind.as_deref() == Some("timeout")
            || state.as_deref() == Some("timeout");
        let access_denied = remote_status_json_bool(status, "access_denied").unwrap_or(false)
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        Self {
            state,
            selected_node: remote_status_json_string(status, "selected_node"),
            selected_peer: remote_status_json_string(status, "selected_peer"),
            failure_kind,
            timed_out,
            access_denied,
            queue_depth: remote_status_json_u64(status, "queue_depth").unwrap_or(0),
            retry_count: remote_status_json_u64(status, "retry_count").unwrap_or(0),
            next_sync_attempt: remote_status_json_i64(status, "next_sync_attempt"),
            last_sync_error: remote_status_json_string(status, "last_sync_error"),
        }
    }
}

fn remote_status_json_bool(value: &JsonValue, key: &str) -> Option<bool> {
    value.get(key).and_then(JsonValue::as_bool)
}

fn remote_status_json_i64(value: &JsonValue, key: &str) -> Option<i64> {
    value.get(key).and_then(JsonValue::as_i64)
}

fn remote_status_json_u64(value: &JsonValue, key: &str) -> Option<u64> {
    value.get(key).and_then(JsonValue::as_u64)
}

fn remote_status_json_string(value: &JsonValue, key: &str) -> Option<String> {
    value.get(key).and_then(JsonValue::as_str).map(ToOwned::to_owned)
}
