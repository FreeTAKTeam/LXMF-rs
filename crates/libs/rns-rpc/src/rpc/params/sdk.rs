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
    updated_ts_ms: u64,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct SdkIdentityBundle {
    identity: String,
    public_key: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
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
    active_identity: Option<String>,
    #[serde(default)]
    remote_commands: HashSet<String>,
    #[serde(default)]
    voice_sessions: HashMap<String, SdkVoiceSessionRecord>,
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
    position: SdkGeoPoint,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkMarkerDeleteV2Params {
    marker_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SdkIdentityListV2Params {
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
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityExportV2Params {
    identity: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityResolveV2Params {
    hash: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkPaperEncodeV2Params {
    message_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkPaperDecodeV2Params {
    uri: String,
    #[serde(default)]
    transient_id: Option<String>,
    #[serde(default)]
    destination_hint: Option<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkCommandInvokeV2Params {
    command: String,
    #[serde(default)]
    target: Option<String>,
    payload: JsonValue,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkCommandReplyV2Params {
    correlation_id: String,
    accepted: bool,
    payload: JsonValue,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkVoiceSessionOpenV2Params {
    peer_id: String,
    #[serde(default)]
    codec_hint: Option<String>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkVoiceSessionUpdateV2Params {
    session_id: String,
    state: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkVoiceSessionCloseV2Params {
    session_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkNegotiateV2Params {
    supported_contract_versions: Vec<u16>,
    #[serde(default)]
    requested_capabilities: Vec<String>,
    config: SdkRuntimeConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkPollEventsV2Params {
    #[serde(default)]
    cursor: Option<String>,
    max: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkCancelMessageV2Params {
    message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkStatusV2Params {
    message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkConfigureV2Params {
    expected_revision: u64,
    patch: JsonValue,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SdkSnapshotV2Params {
    #[serde(default)]
    include_counts: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkShutdownV2Params {
    mode: String,
    #[serde(default)]
    flush_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkRuntimeConfig {
    profile: String,
    #[serde(default)]
    bind_mode: Option<String>,
    #[serde(default)]
    auth_mode: Option<String>,
    #[serde(default)]
    overflow_policy: Option<String>,
    #[serde(default)]
    block_timeout_ms: Option<u64>,
    #[serde(default)]
    rpc_backend: Option<SdkRpcBackendConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkRpcBackendConfig {
    #[serde(default)]
    listen_addr: Option<String>,
    #[serde(default)]
    read_timeout_ms: Option<u64>,
    #[serde(default)]
    write_timeout_ms: Option<u64>,
    #[serde(default)]
    max_header_bytes: Option<usize>,
    #[serde(default)]
    max_body_bytes: Option<usize>,
    #[serde(default)]
    token_auth: Option<SdkTokenAuthConfig>,
    #[serde(default)]
    mtls_auth: Option<SdkMtlsAuthConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkTokenAuthConfig {
    issuer: String,
    audience: String,
    jti_cache_ttl_ms: u64,
    #[serde(default)]
    clock_skew_ms: Option<u64>,
    shared_secret: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkMtlsAuthConfig {
    ca_bundle_path: String,
    require_client_cert: bool,
    #[serde(default)]
    allowed_san: Option<String>,
    #[serde(default)]
    client_cert_path: Option<String>,
    #[serde(default)]
    client_key_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct PropagationNodeRecord {
    peer: String,
    #[serde(default)]
    name: Option<String>,
    last_seen: i64,
    #[serde(default)]
    capabilities: Vec<String>,
    selected: bool,
}
