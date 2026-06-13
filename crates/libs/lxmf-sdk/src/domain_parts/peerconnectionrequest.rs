#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PeerConnectionRequest {
    pub identity: IdentityRef,
    pub display_name: Option<String>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerConnectionState {
    Connected,
    Disconnected,
    Reconnected,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PeerConnectionResult {
    pub identity: IdentityRef,
    pub state: PeerConnectionState,
    pub display_name: Option<String>,
    pub connected: bool,
    pub updated_ts_ms: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}
