#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub struct PropagationNodeSelectionState {
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub selected: bool,
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

impl PropagationNodeSelectionState {
    fn from_peer_and_meta(peer: Option<String>, meta: &JsonValue) -> Self {
        let state = propagation_node_json_string(meta, "state")
            .or_else(|| propagation_node_json_string(meta, "state_name"));
        let failure_kind = propagation_node_json_string(meta, "failure_kind");
        let timed_out = failure_kind.as_deref() == Some("timeout")
            || state.as_deref() == Some("timeout");
        let access_denied = propagation_node_json_bool(meta, "access_denied").unwrap_or(false)
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        let selected = peer.is_some() || propagation_node_json_bool(meta, "selected").unwrap_or(false);
        Self {
            peer,
            state,
            selected,
            failure_kind,
            timed_out,
            access_denied,
            queue_depth: propagation_node_json_u64(meta, "queue_depth").unwrap_or(0),
            retry_count: propagation_node_json_u64(meta, "retry_count").unwrap_or(0),
            next_sync_attempt: propagation_node_json_i64(meta, "next_sync_attempt"),
            last_sync_error: propagation_node_json_string(meta, "last_sync_error"),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationNodeSelectionResult {
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub meta: JsonValue,
    #[serde(default)]
    pub selection_state: PropagationNodeSelectionState,
}

#[derive(Deserialize)]
struct RawPropagationNodeSelectionResult {
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    meta: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationNodeSelectionResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationNodeSelectionResult::deserialize(deserializer)?;
        let selection_state = PropagationNodeSelectionState::from_peer_and_meta(raw.peer.clone(), &raw.meta);
        Ok(Self {
            peer: raw.peer,
            meta: raw.meta,
            selection_state,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub struct PropagationNodeRecord {
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl PropagationNodeRecord {
    fn from_node(node: &JsonValue) -> Self {
        Self {
            peer: propagation_node_json_string(node, "peer"),
            name: propagation_node_json_string(node, "name"),
            last_seen: propagation_node_json_i64(node, "last_seen"),
            selected: propagation_node_json_bool(node, "selected").unwrap_or(false),
            capabilities: propagation_node_json_string_array(node, "capabilities"),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationNodeListResult {
    #[serde(default)]
    pub nodes: Vec<JsonValue>,
    #[serde(default)]
    pub meta: JsonValue,
    #[serde(default)]
    pub node_records: Vec<PropagationNodeRecord>,
}

#[derive(Deserialize)]
struct RawPropagationNodeListResult {
    #[serde(default)]
    nodes: Vec<JsonValue>,
    #[serde(default)]
    meta: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationNodeListResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationNodeListResult::deserialize(deserializer)?;
        let node_records = raw.nodes.iter().map(PropagationNodeRecord::from_node).collect();
        Ok(Self {
            nodes: raw.nodes,
            meta: raw.meta,
            node_records,
        })
    }
}

fn propagation_node_json_bool(value: &JsonValue, key: &str) -> Option<bool> {
    value.get(key).and_then(JsonValue::as_bool)
}

fn propagation_node_json_i64(value: &JsonValue, key: &str) -> Option<i64> {
    value.get(key).and_then(JsonValue::as_i64)
}

fn propagation_node_json_u64(value: &JsonValue, key: &str) -> Option<u64> {
    value.get(key).and_then(JsonValue::as_u64)
}

fn propagation_node_json_string(value: &JsonValue, key: &str) -> Option<String> {
    value.get(key).and_then(JsonValue::as_str).map(ToOwned::to_owned)
}

fn propagation_node_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}
