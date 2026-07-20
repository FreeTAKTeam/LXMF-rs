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
    pub control_allowed: Vec<String>,
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
    /// Projects propagation metadata using the compatibility behavior exposed
    /// before v0.9.6. Malformed optional fields are treated as absent so
    /// existing callers retain the original infallible API.
    pub fn from_propagation(propagation: JsonValue) -> Self {
        let state_name = json_string(&propagation, "state_name").ok().flatten();
        let sync_state =
            json_u64(&propagation, "sync_state").ok().flatten().unwrap_or(0) as u32;
        let failure_kind = json_string(&propagation, "failure_kind")
            .ok()
            .flatten()
            .or_else(|| match state_name.as_deref() {
                Some("no_access") => Some("no_access".to_string()),
                Some("timeout") => Some("timeout".to_string()),
                _ => None,
            });
        let timed_out =
            failure_kind.as_deref() == Some("timeout") || state_name.as_deref() == Some("timeout");
        let access_denied = json_bool(&propagation, "access_denied")
            .ok()
            .flatten()
            .unwrap_or(false)
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
            stamp_cost_flexibility: json_u64(&propagation, "stamp_cost_flexibility")
                .ok()
                .flatten(),
            message_storage_limit_mb: json_u64(&propagation, "message_storage_limit_mb")
                .ok()
                .flatten(),
            delivery_limit: json_u64(&propagation, "delivery_limit").ok().flatten(),
            propagation_limit: json_u64(&propagation, "propagation_limit").ok().flatten(),
            autopeer: json_bool(&propagation, "autopeer").ok().flatten(),
            autopeer_maxdepth: json_u64(&propagation, "autopeer_maxdepth").ok().flatten(),
            static_peers: remote_transfer_json_string_array(&propagation, "static_peers"),
            sync_limit: json_u64(&propagation, "sync_limit").ok().flatten(),
            max_peers: json_u64(&propagation, "max_peers").ok().flatten(),
            from_static_only: json_bool(&propagation, "from_static_only").ok().flatten(),
            retain_synced_on_node: json_bool(&propagation, "retain_synced_on_node")
                .ok()
                .flatten(),
            peering_cost: json_u64(&propagation, "peering_cost").ok().flatten(),
            remote_peering_cost_max: json_u64(&propagation, "remote_peering_cost_max")
                .ok()
                .flatten(),
            control_allowed: remote_transfer_json_string_array(&propagation, "control_allowed"),
            total_ingested: json_u64(&propagation, "total_ingested")
                .ok()
                .flatten()
                .unwrap_or(0),
            last_ingest_count: json_u64(&propagation, "last_ingest_count")
                .ok()
                .flatten()
                .unwrap_or(0),
            client_propagation_messages_received: json_u64(
                &propagation,
                "client_propagation_messages_received",
            )
            .ok()
            .flatten()
            .unwrap_or(0),
            client_propagation_messages_served: json_u64(
                &propagation,
                "client_propagation_messages_served",
            )
            .ok()
            .flatten()
            .unwrap_or(0),
            propagation,
        }
    }

    /// Strictly projects propagation metadata and reports malformed typed
    /// fields instead of silently treating them as absent.
    pub fn try_from_propagation(propagation: JsonValue) -> Result<Self, String> {
        macro_rules! field {
            ($getter:ident, $key:literal) => {
                $getter(&propagation, $key)
                    .map_err(|error| format!("propagation field `{}` {error}", $key))?
            };
        }

        let state_name = field!(json_string, "state_name");
        let sync_state = u32::try_from(field!(json_u64, "sync_state").unwrap_or(0))
            .map_err(|_| "propagation field `sync_state` exceeds u32 range".to_string())?;
        let failure_kind = field!(json_string, "failure_kind").or_else(|| match state_name.as_deref() {
            Some("no_access") => Some("no_access".to_string()),
            Some("timeout") => Some("timeout".to_string()),
            _ => None,
        });
        let timed_out = failure_kind.as_deref() == Some("timeout") || state_name.as_deref() == Some("timeout");
        let access_denied = field!(json_bool, "access_denied").unwrap_or(false)
            || state_name.as_deref() == Some("no_access")
            || sync_state == 0xf4
            || matches!(
                failure_kind.as_deref(),
                Some("access_denied" | "access-denied" | "no_access")
            );
        Ok(Self {
            enabled: field!(json_bool, "enabled").unwrap_or(false),
            selected_node: field!(json_string, "selected_node"),
            sync_state,
            state_name,
            sync_progress: field!(json_f64, "sync_progress"),
            last_sync_started: field!(json_i64, "last_sync_started"),
            last_sync_completed: field!(json_i64, "last_sync_completed"),
            last_sync_error: field!(json_string, "last_sync_error"),
            failure_kind,
            timed_out,
            access_denied,
            next_sync_attempt: field!(json_i64, "next_sync_attempt"),
            retry_count: field!(json_u64, "retry_count").unwrap_or(0),
            queue_depth: field!(json_u64, "queue_depth").unwrap_or(0),
            timestamp: field!(json_i64, "timestamp"),
            auth_required: field!(json_bool, "auth_required").unwrap_or(false),
            store_root: field!(json_string, "store_root"),
            target_cost: field!(json_u64, "target_cost"),
            stamp_cost_flexibility: field!(json_u64, "stamp_cost_flexibility"),
            message_storage_limit_mb: field!(json_u64, "message_storage_limit_mb"),
            delivery_limit: field!(json_u64, "delivery_limit"),
            propagation_limit: field!(json_u64, "propagation_limit"),
            autopeer: field!(json_bool, "autopeer"),
            autopeer_maxdepth: field!(json_u64, "autopeer_maxdepth"),
            static_peers: json_string_array(&propagation, "static_peers")?,
            sync_limit: field!(json_u64, "sync_limit"),
            max_peers: field!(json_u64, "max_peers"),
            from_static_only: field!(json_bool, "from_static_only"),
            retain_synced_on_node: field!(json_bool, "retain_synced_on_node"),
            peering_cost: field!(json_u64, "peering_cost"),
            remote_peering_cost_max: field!(json_u64, "remote_peering_cost_max"),
            control_allowed: json_string_array(&propagation, "control_allowed")?,
            total_ingested: field!(json_u64, "total_ingested").unwrap_or(0),
            last_ingest_count: field!(json_u64, "last_ingest_count").unwrap_or(0),
            client_propagation_messages_received: field!(
                json_u64,
                "client_propagation_messages_received"
            )
            .unwrap_or(0),
            client_propagation_messages_served: field!(
                json_u64,
                "client_propagation_messages_served"
            )
            .unwrap_or(0),
            propagation,
        })
    }
}

fn json_bool(value: &JsonValue, key: &str) -> Result<Option<bool>, &'static str> {
    match value.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => v.as_bool().ok_or("field is not a bool").map(Some),
    }
}

fn json_f64(value: &JsonValue, key: &str) -> Result<Option<f64>, &'static str> {
    match value.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => v.as_f64().ok_or("field is not a number").map(Some),
    }
}

fn json_i64(value: &JsonValue, key: &str) -> Result<Option<i64>, &'static str> {
    match value.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => v.as_i64().ok_or("field is not an integer").map(Some),
    }
}

fn json_u64(value: &JsonValue, key: &str) -> Result<Option<u64>, &'static str> {
    match value.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => v.as_u64().ok_or("field is not an unsigned integer").map(Some),
    }
}

fn json_string(value: &JsonValue, key: &str) -> Result<Option<String>, &'static str> {
    match value.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => v.as_str().ok_or("field is not a string").map(|s| Some(s.to_owned())),
    }
}

fn json_string_array(value: &JsonValue, key: &str) -> Result<Vec<String>, String> {
    let Some(raw) = value.get(key) else {
        return Ok(Vec::new());
    };
    if raw.is_null() {
        return Ok(Vec::new());
    }
    let items = raw.as_array().ok_or_else(|| format!("propagation field `{key}` is not an array"))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("propagation field `{key}` contains a non-string value"))
        })
        .collect()
}

fn remote_transfer_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod propagation_recovery_state_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recovery_state_accepts_absent_and_null_optional_fields() {
        let state = PropagationRecoveryStateResult::try_from_propagation(json!({
            "enabled": true,
            "selected_node": null,
            "static_peers": null
        }))
        .expect("valid recovery state");

        assert!(state.enabled);
        assert_eq!(state.selected_node, None);
        assert!(state.static_peers.is_empty());
    }

    #[test]
    fn recovery_state_rejects_malformed_typed_fields() {
        let error = PropagationRecoveryStateResult::try_from_propagation(json!({
            "queue_depth": "many"
        }))
        .expect_err("invalid queue depth");
        assert!(error.contains("queue_depth"));

        let error = PropagationRecoveryStateResult::try_from_propagation(json!({
            "sync_state": u64::from(u32::MAX) + 1
        }))
        .expect_err("overflowing sync state");
        assert!(error.contains("sync_state"));

        let error = PropagationRecoveryStateResult::try_from_propagation(json!({
            "static_peers": ["peer-a", 42]
        }))
        .expect_err("invalid static peer entry");
        assert!(error.contains("static_peers"));
    }

    #[test]
    fn compatibility_recovery_state_keeps_infallible_projection() {
        let state = PropagationRecoveryStateResult::from_propagation(json!({
            "enabled": true,
            "queue_depth": "many",
            "static_peers": ["peer-a", 42]
        }));

        assert!(state.enabled);
        assert_eq!(state.queue_depth, 0);
        assert_eq!(state.static_peers, vec!["peer-a".to_string()]);
    }
}
