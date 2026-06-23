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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationNodeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub peer_announce_at_start: bool,
    #[serde(default)]
    pub peer_announce_interval_secs: Option<u64>,
    #[serde(default)]
    pub node_announce_at_start: bool,
    #[serde(default)]
    pub node_announce_interval_secs: Option<u64>,
    #[serde(default = "default_propagation_node_transfer_limit_kb")]
    pub transfer_limit_kb: u32,
    #[serde(default = "default_propagation_node_sync_limit_kb")]
    pub sync_limit_kb: u32,
    #[serde(default = "default_propagation_node_stamp_cost")]
    pub stamp_cost: u32,
    #[serde(default = "default_propagation_node_stamp_cost_flexibility")]
    pub stamp_cost_flexibility: u32,
    #[serde(default = "default_propagation_node_peering_cost")]
    pub peering_cost: u32,
    #[serde(default)]
    pub control_allowed: Vec<String>,
}

impl Default for PropagationNodeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            peer_announce_at_start: false,
            peer_announce_interval_secs: None,
            node_announce_at_start: false,
            node_announce_interval_secs: None,
            transfer_limit_kb: default_propagation_node_transfer_limit_kb(),
            sync_limit_kb: default_propagation_node_sync_limit_kb(),
            stamp_cost: default_propagation_node_stamp_cost(),
            stamp_cost_flexibility: default_propagation_node_stamp_cost_flexibility(),
            peering_cost: default_propagation_node_peering_cost(),
            control_allowed: Vec::new(),
        }
    }
}

impl PropagationNodeConfig {
    fn from_meta(meta: &JsonValue) -> Self {
        meta.get("propagation_node")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }
}

impl PropagationNodeSelectionState {
    fn from_peer_and_meta(peer: Option<String>, meta: &JsonValue) -> Self {
        let state = propagation_node_json_string(meta, "state").ok().flatten()
            .or_else(|| propagation_node_json_string(meta, "state_name").ok().flatten());
        let failure_kind = propagation_node_json_string(meta, "failure_kind").ok().flatten();
        let timed_out = failure_kind.as_deref() == Some("timeout")
            || state.as_deref() == Some("timeout");
        let access_denied = propagation_node_json_bool(meta, "access_denied").ok().flatten().unwrap_or(false)
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        let selected = peer.is_some() || propagation_node_json_bool(meta, "selected").ok().flatten().unwrap_or(false);
        Self {
            peer,
            state,
            selected,
            failure_kind,
            timed_out,
            access_denied,
            queue_depth: propagation_node_json_u64(meta, "queue_depth").ok().flatten().unwrap_or(0),
            retry_count: propagation_node_json_u64(meta, "retry_count").ok().flatten().unwrap_or(0),
            next_sync_attempt: propagation_node_json_i64(meta, "next_sync_attempt").ok().flatten(),
            last_sync_error: propagation_node_json_string(meta, "last_sync_error").ok().flatten(),
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
    #[serde(default)]
    pub node_config: PropagationNodeConfig,
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
        let node_config = PropagationNodeConfig::from_meta(&raw.meta);
        Ok(Self {
            peer: raw.peer,
            meta: raw.meta,
            selection_state,
            node_config,
        })
    }
}

fn default_propagation_node_transfer_limit_kb() -> u32 {
    256
}

fn default_propagation_node_sync_limit_kb() -> u32 {
    10240
}

fn default_propagation_node_stamp_cost() -> u32 {
    16
}

fn default_propagation_node_stamp_cost_flexibility() -> u32 {
    3
}

fn default_propagation_node_peering_cost() -> u32 {
    18
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
            peer: propagation_node_json_string(node, "peer").ok().flatten(),
            name: propagation_node_json_string(node, "name").ok().flatten(),
            last_seen: propagation_node_json_i64(node, "last_seen").ok().flatten(),
            selected: propagation_node_json_bool(node, "selected").ok().flatten().unwrap_or(false),
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

fn propagation_node_json_bool(value: &JsonValue, key: &str) -> Result<Option<bool>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_bool().ok_or("field is not a bool").map(Some),
    }
}

fn propagation_node_json_i64(value: &JsonValue, key: &str) -> Result<Option<i64>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_i64().ok_or("field is not an integer").map(Some),
    }
}

fn propagation_node_json_u64(value: &JsonValue, key: &str) -> Result<Option<u64>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_u64().ok_or("field is not an unsigned integer").map(Some),
    }
}

fn propagation_node_json_string(value: &JsonValue, key: &str) -> Result<Option<String>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_str().ok_or("field is not a string").map(|s| Some(s.to_owned())),
    }
}

fn propagation_node_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod propagation_node_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn node_get_exposes_propagation_node_config_from_meta() {
        let result: PropagationNodeSelectionResult = serde_json::from_value(json!({
            "peer": null,
            "meta": {
                "propagation_node": {
                    "enabled": true,
                    "peer_announce_at_start": true,
                    "peer_announce_interval_secs": 120,
                    "node_announce_at_start": true,
                    "node_announce_interval_secs": 300,
                    "transfer_limit_kb": 512,
                    "sync_limit_kb": 20480,
                    "stamp_cost": 21,
                    "stamp_cost_flexibility": 4,
                    "peering_cost": 23,
                    "control_allowed": ["00112233445566778899aabbccddeeff"]
                }
            }
        }))
        .expect("decode node selection");

        assert!(result.node_config.enabled);
        assert!(result.node_config.peer_announce_at_start);
        assert_eq!(result.node_config.peer_announce_interval_secs, Some(120));
        assert!(result.node_config.node_announce_at_start);
        assert_eq!(result.node_config.node_announce_interval_secs, Some(300));
        assert_eq!(result.node_config.transfer_limit_kb, 512);
        assert_eq!(result.node_config.sync_limit_kb, 20480);
        assert_eq!(result.node_config.stamp_cost, 21);
        assert_eq!(result.node_config.stamp_cost_flexibility, 4);
        assert_eq!(result.node_config.peering_cost, 23);
        assert_eq!(
            result.node_config.control_allowed,
            vec!["00112233445566778899aabbccddeeff".to_string()]
        );
    }
}
