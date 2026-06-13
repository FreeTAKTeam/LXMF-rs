#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteRequest {
    pub remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_private_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_limit_kb: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemotePeerRequest {
    pub remote: String,
    pub peer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_private_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_limit_kb: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteStatusResult {
    pub remote: String,
    #[serde(default)]
    pub status: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteTransferResult {
    pub remote: String,
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteSyncResult {
    pub remote: String,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub propagation: JsonValue,
    #[serde(default)]
    pub peer_sync: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationRemoteUnpeerResult {
    pub remote: String,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub propagation_cleared: Option<u64>,
    #[serde(default)]
    pub propagation_cleared_bytes: Option<u64>,
    #[serde(default)]
    pub messages: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
}
