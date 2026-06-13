#[derive(Clone, Debug, Serialize, PartialEq)]
#[non_exhaustive]
pub struct PropagationPeerMaintenanceResult {
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub culled: u64,
    #[serde(default)]
    pub culled_peers: Vec<String>,
    #[serde(default)]
    pub rotated: u64,
    #[serde(default)]
    pub rotated_peers: Vec<String>,
    #[serde(default)]
    pub synced_peer: Option<String>,
    #[serde(default)]
    pub peer_sync: JsonValue,
    #[serde(default)]
    pub peer_sync_state: Option<PropagationPeerSyncResult>,
    #[serde(default)]
    pub max_unreachable_secs: u64,
}

#[derive(Deserialize)]
struct RawPropagationPeerMaintenanceResult {
    #[serde(default)]
    timestamp: i64,
    #[serde(default)]
    culled: u64,
    #[serde(default)]
    culled_peers: Vec<String>,
    #[serde(default)]
    rotated: u64,
    #[serde(default)]
    rotated_peers: Vec<String>,
    #[serde(default)]
    synced_peer: Option<String>,
    #[serde(default)]
    peer_sync: JsonValue,
    #[serde(default)]
    max_unreachable_secs: u64,
}

impl<'de> Deserialize<'de> for PropagationPeerMaintenanceResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPropagationPeerMaintenanceResult::deserialize(deserializer)?;
        let peer_sync_state = if raw.peer_sync.get("peer").is_some() {
            Some(
                serde_json::from_value::<PropagationPeerSyncResult>(raw.peer_sync.clone())
                    .map_err(serde::de::Error::custom)?,
            )
        } else {
            None
        };
        Ok(Self {
            timestamp: raw.timestamp,
            culled: raw.culled,
            culled_peers: raw.culled_peers,
            rotated: raw.rotated,
            rotated_peers: raw.rotated_peers,
            synced_peer: raw.synced_peer,
            peer_sync: raw.peer_sync,
            peer_sync_state,
            max_unreachable_secs: raw.max_unreachable_secs,
        })
    }
}
