#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkTopicRecord {
    topic_id: String,
    #[serde(default)]
    topic_path: Option<String>,
    created_ts_ms: u64,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkTelemetryPoint {
    ts_ms: u64,
    key: String,
    value: JsonValue,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkAttachmentRecord {
    attachment_id: String,
    name: String,
    content_type: String,
    byte_len: u64,
    checksum_sha256: String,
    created_ts_ms: u64,
    #[serde(default)]
    expires_ts_ms: Option<u64>,
    #[serde(default)]
    topic_ids: Vec<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
struct SdkAttachmentUploadSession {
    upload_id: String,
    attachment_id: String,
    name: String,
    content_type: String,
    total_size: u64,
    checksum_sha256: String,
    expires_ts_ms: Option<u64>,
    topic_ids: Vec<String>,
    extensions: JsonMap<String, JsonValue>,
    payload: Vec<u8>,
    next_offset: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkGeoPoint {
    lat: f64,
    lon: f64,
    #[serde(default)]
    alt_m: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkMarkerRecord {
    marker_id: String,
    label: String,
    position: SdkGeoPoint,
    #[serde(default)]
    topic_id: Option<String>,
    #[serde(default = "sdk_default_marker_revision")]
    revision: u64,
    updated_ts_ms: u64,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkIdentityBundle {
    identity: String,
    #[serde(default)]
    delivery_destination: Option<String>,
    public_key: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkContactRecord {
    identity: String,
    #[serde(default)]
    display_name: Option<String>,
    trust_level: String,
    bootstrap: bool,
    updated_ts_ms: u64,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkPresenceRecord {
    peer_id: String,
    last_seen_ts_ms: i64,
    first_seen_ts_ms: i64,
    seen_count: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    name_source: Option<String>,
    #[serde(default)]
    trust_level: Option<String>,
    #[serde(default)]
    bootstrap: Option<bool>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkVoiceSessionRecord {
    session_id: String,
    peer_id: String,
    #[serde(default)]
    codec_hint: Option<String>,
    state: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
struct SdkDomainSnapshotV1 {
    #[serde(default)]
    next_domain_seq: u64,
    #[serde(default)]
    config_revision: u64,
    #[serde(default)]
    runtime_config: JsonValue,
    #[serde(default)]
    topics: HashMap<String, SdkTopicRecord>,
    #[serde(default)]
    topic_order: Vec<String>,
    #[serde(default)]
    topic_subscriptions: HashSet<String>,
    #[serde(default)]
    telemetry_points: Vec<SdkTelemetryPoint>,
    #[serde(default)]
    attachments: HashMap<String, SdkAttachmentRecord>,
    #[serde(default)]
    attachment_payloads: HashMap<String, String>,
    #[serde(default)]
    attachment_order: Vec<String>,
    #[serde(default)]
    markers: HashMap<String, SdkMarkerRecord>,
    #[serde(default)]
    marker_order: Vec<String>,
    #[serde(default)]
    identities: HashMap<String, SdkIdentityBundle>,
    #[serde(default)]
    contacts: HashMap<String, SdkContactRecord>,
    #[serde(default)]
    contact_order: Vec<String>,
    #[serde(default)]
    active_identity: Option<String>,
    #[serde(default, deserialize_with = "deserialize_remote_commands")]
    remote_commands: HashMap<String, SdkRemoteCommandRecord>,
    #[serde(default)]
    voice_sessions: HashMap<String, SdkVoiceSessionRecord>,
    #[serde(default)]
    blackholed_identities: HashMap<String, JsonValue>,
}

fn deserialize_remote_commands<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, SdkRemoteCommandRecord>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = JsonValue::deserialize(deserializer)?;
    match value {
        JsonValue::Null => Ok(HashMap::new()),
        JsonValue::Object(map) => serde_json::from_value(JsonValue::Object(map))
            .map_err(serde::de::Error::custom),
        JsonValue::Array(_) => Ok(HashMap::new()),
        other => Err(serde::de::Error::custom(format!(
            "remote_commands must be object or array, got {other}"
        ))),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkRemoteCommandRecord {
    command_id: String,
    correlation_id: String,
    command: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    delivery_state: Option<String>,
    command_state: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    request_payload: JsonValue,
    #[serde(default)]
    response_payload: Option<JsonValue>,
    #[serde(default)]
    accepted: Option<bool>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTopicCreateV2Params {
    #[serde(default)]
    topic_path: Option<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTopicGetV2Params {
    topic_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTopicListV2Params {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTopicSubscriptionV2Params {
    topic_id: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTopicPublishV2Params {
    topic_id: String,
    payload: JsonValue,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTelemetryQueryV2Params {
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    topic_id: Option<String>,
    #[serde(default)]
    from_ts_ms: Option<u64>,
    #[serde(default)]
    to_ts_ms: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentStoreV2Params {
    name: String,
    content_type: String,
    bytes_base64: String,
    #[serde(default)]
    expires_ts_ms: Option<u64>,
    #[serde(default)]
    topic_ids: Vec<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentRefV2Params {
    attachment_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentListV2Params {
    #[serde(default)]
    topic_id: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentAssociateTopicV2Params {
    attachment_id: String,
    topic_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentUploadStartV2Params {
    name: String,
    content_type: String,
    total_size: u64,
    checksum_sha256: String,
    #[serde(default)]
    expires_ts_ms: Option<u64>,
    #[serde(default)]
    topic_ids: Vec<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentUploadChunkV2Params {
    upload_id: String,
    offset: u64,
    bytes_base64: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentUploadCommitV2Params {
    upload_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkAttachmentDownloadChunkV2Params {
    attachment_id: String,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkMarkerCreateV2Params {
    label: String,
    position: SdkGeoPoint,
    #[serde(default)]
    topic_id: Option<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkMarkerListV2Params {
    #[serde(default)]
    topic_id: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkMarkerUpdatePositionV2Params {
    marker_id: String,
    expected_revision: u64,
    position: SdkGeoPoint,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkMarkerDeleteV2Params {
    marker_id: String,
    expected_revision: u64,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

fn sdk_default_marker_revision() -> u64 {
    1
}

fn sdk_default_identity_bootstrap_auto_sync() -> bool {
    true
}
