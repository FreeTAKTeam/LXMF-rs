#[derive(Clone, Debug, Serialize, PartialEq)]
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
    pub propagation: JsonValue,
    #[serde(default)]
    pub messages: JsonValue,
    #[serde(default)]
    pub result: JsonValue,
    #[serde(default)]
    pub transfer_state: PropagationRemoteTransferState,
    #[serde(default)]
    pub queue: PropagationPeerQueueSnapshot,
}

#[derive(Deserialize)]
struct RawPropagationRemoteUnpeerResult {
    remote: String,
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    removed: bool,
    #[serde(default)]
    propagation_cleared: Option<u64>,
    #[serde(default)]
    propagation_cleared_bytes: Option<u64>,
    #[serde(default)]
    propagation: JsonValue,
    #[serde(default)]
    messages: JsonValue,
    #[serde(default)]
    result: JsonValue,
}

impl<'de> Deserialize<'de> for PropagationRemoteUnpeerResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationRemoteUnpeerResult::deserialize(deserializer)?;
        let queue =
            PropagationPeerQueueSnapshot::from_messages_and_propagation(&raw.messages, &raw.propagation);
        let transfer_state =
            PropagationRemoteTransferState::from_result_and_propagation(&raw.result, &raw.propagation);
        Ok(Self {
            remote: raw.remote,
            peer: raw.peer,
            removed: raw.removed,
            propagation_cleared: raw.propagation_cleared,
            propagation_cleared_bytes: raw.propagation_cleared_bytes,
            propagation: raw.propagation,
            messages: raw.messages,
            result: raw.result,
            transfer_state,
            queue,
        })
    }
}
