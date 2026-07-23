#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct IdentityCreateRequest {
    pub display_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}
