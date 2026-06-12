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
struct SdkIdentityContactUpdateV2Params {
    identity: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    trust_level: Option<String>,
    #[serde(default)]
    bootstrap: Option<bool>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityContactListV2Params {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkIdentityBootstrapV2Params {
    identity: String,
    #[serde(default = "sdk_default_identity_bootstrap_auto_sync")]
    auto_sync: bool,
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
struct SdkCommandSessionGetV2Params {
    correlation_id: String,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkCommandSessionListV2Params {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkOperationRegistryV2Params {
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkEnvelopeExecuteV2Params {
    operation_id: String,
    kind: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
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
    store_forward: Option<SdkStoreForwardConfig>,
    #[serde(default)]
    event_sink: Option<SdkEventSinkConfig>,
    #[serde(default)]
    rpc_backend: Option<SdkRpcBackendConfig>,
    #[serde(default)]
    extensions: JsonMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkStoreForwardConfig {
    #[serde(default)]
    max_messages: Option<usize>,
    #[serde(default)]
    max_message_age_ms: Option<u64>,
    #[serde(default)]
    capacity_policy: Option<String>,
    #[serde(default)]
    eviction_priority: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SdkEventSinkConfig {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    max_event_bytes: Option<u64>,
    #[serde(default)]
    allow_kinds: Option<Vec<String>>,
    #[serde(default)]
    extensions: Option<JsonMap<String, JsonValue>>,
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
