#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationDeliveryPolicyResult {
    #[serde(default)]
    pub policy: JsonValue,
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
