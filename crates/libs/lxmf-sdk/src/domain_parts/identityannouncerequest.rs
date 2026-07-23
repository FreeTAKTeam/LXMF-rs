#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct IdentityAnnounceRequest {
    pub identity: Option<IdentityRef>,
    pub display_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IdentityAnnounceResult {
    pub accepted: bool,
    pub announce_id: Option<JsonValue>,
    pub identity: Option<IdentityRef>,
    pub delivery_destination: Option<String>,
    pub display_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}
