#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SdkIdentityListV2Params {
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SdkIdentityAnnounceNowV2Params {
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SdkIdentityCreateV2Params {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityPresenceListV2Params {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    min_last_seen_ts_ms: Option<i64>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkPeerConnectionV2Params {
    identity: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityActivateV2Params {
    identity: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityImportV2Params {
    bundle_base64: String,
    #[serde(default)]
    passphrase: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkPrivateIdentityBundleV1 {
    version: u8,
    private_key_base64: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
}
