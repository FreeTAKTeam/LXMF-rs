#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub struct PropagationDeliveryPolicyState {
    #[serde(default)]
    pub auth_required: bool,
    #[serde(default)]
    pub allowed_destinations: Vec<String>,
    #[serde(default)]
    pub denied_destinations: Vec<String>,
    #[serde(default)]
    pub ignored_destinations: Vec<String>,
    #[serde(default)]
    pub prioritised_destinations: Vec<String>,
}

impl PropagationDeliveryPolicyState {
    fn from_policy(policy: &JsonValue) -> Self {
        Self {
            auth_required: propagation_policy_json_bool(policy, "auth_required").ok().flatten().unwrap_or(false),
            allowed_destinations: propagation_policy_json_string_array(policy, "allowed_destinations"),
            denied_destinations: propagation_policy_json_string_array(policy, "denied_destinations"),
            ignored_destinations: propagation_policy_json_string_array(policy, "ignored_destinations"),
            prioritised_destinations: propagation_policy_json_string_array(
                policy,
                "prioritised_destinations",
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationDeliveryPolicyResult {
    #[serde(default)]
    pub policy: JsonValue,
    #[serde(default)]
    pub policy_state: PropagationDeliveryPolicyState,
}

#[derive(Deserialize)]
struct RawPropagationDeliveryPolicyResult {
    #[serde(default)]
    policy: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationDeliveryPolicyResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationDeliveryPolicyResult::deserialize(deserializer)?;
        let policy_state = PropagationDeliveryPolicyState::from_policy(&raw.policy);
        Ok(Self {
            policy: raw.policy,
            policy_state,
        })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
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
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub recovery_state: PropagationRecoveryStateResult,
}

#[derive(Deserialize)]
struct RawPropagationIngestResult {
    #[serde(default)]
    ingested_count: u64,
    #[serde(default)]
    duplicate_count: u64,
    #[serde(default)]
    payload_bytes: u64,
    #[serde(default)]
    transferred_bytes: u64,
    #[serde(default)]
    transient_id: String,
    #[serde(default)]
    propagation: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationIngestResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationIngestResult::deserialize(deserializer)?;
        let recovery_state = PropagationRecoveryStateResult::from_propagation(raw.propagation.clone());
        Ok(Self {
            ingested_count: raw.ingested_count,
            duplicate_count: raw.duplicate_count,
            payload_bytes: raw.payload_bytes,
            transferred_bytes: raw.transferred_bytes,
            transient_id: raw.transient_id,
            propagation: raw.propagation,
            recovery_state,
        })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
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
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub recovery_state: PropagationRecoveryStateResult,
}

#[derive(Deserialize)]
struct RawPropagationFetchResult {
    #[serde(default)]
    transient_id: String,
    #[serde(default)]
    payload_hex: String,
    #[serde(default)]
    payload_bytes: u64,
    #[serde(default)]
    transferred_bytes: u64,
    #[serde(default)]
    propagation: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationFetchResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationFetchResult::deserialize(deserializer)?;
        let recovery_state = PropagationRecoveryStateResult::from_propagation(raw.propagation.clone());
        Ok(Self {
            transient_id: raw.transient_id,
            payload_hex: raw.payload_hex,
            payload_bytes: raw.payload_bytes,
            transferred_bytes: raw.transferred_bytes,
            propagation: raw.propagation,
            recovery_state,
        })
    }
}

fn propagation_policy_json_bool(value: &JsonValue, key: &str) -> Result<Option<bool>, &'static str> {
    match value.get(key) {
        None => Ok(None),
        Some(v) => v.as_bool().ok_or("field is not a bool").map(Some),
    }
}

fn propagation_policy_json_string_array(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}
