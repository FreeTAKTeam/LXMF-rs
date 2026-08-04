#[derive(Debug, Deserialize)]
struct RecordReceiptParams {
    message_id: String,
    status: String,
    #[serde(default)]
    packet_hash: Option<String>,
    #[serde(default)]
    resource_hash: Option<String>,
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    delivery_kind: Option<String>,
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    link_id: Option<String>,
    #[serde(default)]
    stage: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PacketHashParams {
    packet_hash: String,
}

#[derive(Debug, Deserialize)]
struct RouterStoragePolicyParams {
    #[serde(default)]
    message_limit_bytes: Option<u64>,
    #[serde(default)]
    information_limit_bytes: Option<u64>,
    #[serde(default)]
    retain_node_lxms: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ReceiveMessageParams {
    id: String,
    source: String,
    destination: String,
    #[serde(default)]
    title: String,
    content: String,
    fields: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct AnnounceReceivedParams {
    peer: String,
    timestamp: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    name_source: Option<String>,
    #[serde(default)]
    app_data_hex: Option<String>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    rssi: Option<f64>,
    #[serde(default)]
    snr: Option<f64>,
    #[serde(default)]
    q: Option<f64>,
    #[serde(default)]
    stamp_cost: Option<u32>,
    #[serde(default)]
    stamp_cost_flexibility: Option<u32>,
    #[serde(default)]
    peering_cost: Option<u32>,
    #[serde(default)]
    aspect: Option<String>,
    #[serde(default)]
    hops: Option<u32>,
    #[serde(default)]
    interface: Option<String>,
    #[serde(default)]
    source_private_key: Option<String>,
    #[serde(default)]
    source_identity: Option<String>,
    #[serde(default)]
    source_node: Option<String>,
    #[serde(default)]
    is_path_response: bool,
}

#[derive(Debug, Deserialize)]
struct AnnounceDeliveryParams {
    #[serde(alias = "destination", alias = "hash")]
    destination_hash: String,
}

#[derive(Debug, Deserialize)]
struct BlackholeIdentityParams {
    #[serde(alias = "identity_hash", alias = "identity", alias = "hash")]
    identity: String,
    #[serde(default)]
    until: Option<JsonValue>,
    #[serde(default)]
    reason: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct SetInterfacesParams {
    interfaces: Vec<InterfaceRecord>,
}

#[derive(Debug, Deserialize)]
struct ReloadConfigParams {
    interfaces: Vec<InterfaceRecord>,
}

#[derive(Debug, Deserialize)]
struct PeerOpParams {
    peer: String,
    #[serde(default, deserialize_with = "deserialize_python_transfer_limit_kb")]
    transfer_limit_kb: Option<f64>,
    #[serde(default)]
    wanted_ids: Option<JsonValue>,
    #[serde(default)]
    maintenance_claimed: bool,
    #[serde(default)]
    force_sync: bool,
}

#[derive(Debug, Deserialize)]
struct DeliveryPolicyParams {
    #[serde(default)]
    auth_required: Option<bool>,
    #[serde(default)]
    allowed_destinations: Option<Vec<String>>,
    #[serde(default)]
    denied_destinations: Option<Vec<String>>,
    #[serde(default)]
    ignored_destinations: Option<Vec<String>>,
    #[serde(default)]
    prioritised_destinations: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DeliveryPolicyHashMutationParams {
    #[serde(
        default,
        alias = "identity_hash",
        alias = "destination_hash",
        alias = "identity",
        alias = "hash"
    )]
    destination: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeliveryPolicyAuthenticationParams {
    #[serde(default, alias = "auth_required", alias = "required")]
    authentication_required: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DeliveryPolicyEntryParams {
    #[serde(alias = "destination_hash", alias = "destination", alias = "hash")]
    destination: String,
}

#[derive(Debug, Deserialize)]
struct PropagationEnableParams {
    enabled: bool,
    #[serde(default)]
    auth_required: Option<bool>,
    #[serde(default)]
    store_root: Option<String>,
    #[serde(default)]
    target_cost: Option<u32>,
    #[serde(default)]
    stamp_cost_flexibility: Option<u32>,
    #[serde(default)]
    message_storage_limit_mb: Option<u64>,
    #[serde(default)]
    peer_entry_limit: Option<u64>,
    #[serde(default)]
    peer_entry_limit_per_peer: Option<u64>,
    #[serde(default)]
    peer_entry_ttl_secs: Option<u64>,
    #[serde(default)]
    completed_peer_entry_ttl_secs: Option<u64>,
    #[serde(default)]
    max_propagation_peers: Option<u32>,
    #[serde(default)]
    storage_maintenance_interval_secs: Option<u64>,
    #[serde(default)]
    delivery_limit: Option<u32>,
    #[serde(default)]
    propagation_limit: Option<u32>,
    #[serde(default)]
    sync_limit: Option<u32>,
    #[serde(default)]
    autopeer: Option<bool>,
    #[serde(default)]
    autopeer_maxdepth: Option<u32>,
    #[serde(default)]
    static_peers: Option<Vec<String>>,
    #[serde(default)]
    max_peers: Option<u32>,
    #[serde(default)]
    from_static_only: Option<bool>,
    #[serde(default)]
    retain_synced_on_node: Option<bool>,
    #[serde(default)]
    peering_cost: Option<u32>,
    #[serde(default)]
    remote_peering_cost_max: Option<u32>,
    #[serde(default)]
    control_allowed: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct PropagationControlAclParams {
    #[serde(alias = "identity", alias = "identity_hash", alias = "hash")]
    identity_hash: String,
}

#[derive(Debug, Deserialize)]
struct PropagationIngestParams {
    #[serde(default)]
    transient_id: Option<String>,
    #[serde(default)]
    payload_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PropagationFetchParams {
    transient_id: String,
}

#[derive(Debug, Deserialize)]
struct PaperIngestUriParams {
    uri: String,
}

#[derive(Debug, Deserialize)]
struct StampPolicySetParams {
    #[serde(default)]
    target_cost: Option<u32>,
    #[serde(default)]
    flexibility: Option<u32>,
    #[serde(default)]
    enforce: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OutboundStampCostParams {
    #[serde(alias = "destination_hash", alias = "hash")]
    destination: String,
}

#[derive(Debug, Deserialize)]
struct TicketGenerateParams {
    destination: String,
    #[serde(default)]
    ttl_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RNodeManagementParams {
    iface: String,
    command: String,
}

#[derive(Debug, Deserialize)]
struct WeaveRemoteDisplayControlParams {
    iface: String,
    enable: bool,
    #[serde(default)]
    remote_switch_id_hex: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ListMessagesParams {
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    include_receipts: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_ts: Option<i64>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ListConversationsParams {
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ListAnnouncesParams {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_ts: Option<i64>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetOutboundPropagationNodeParams {
    #[serde(default, alias = "destination_hash", alias = "destination", alias = "hash")]
    peer: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PropagationRemoteStatusParams {
    remote: String,
    #[serde(default)]
    identity_private_key_hex: Option<String>,
    #[serde(default)]
    timeout_secs: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_python_transfer_limit_kb")]
    transfer_limit_kb: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PropagationRemotePeerParams {
    remote: String,
    peer: String,
    #[serde(default)]
    identity_private_key_hex: Option<String>,
    #[serde(default)]
    timeout_secs: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_python_transfer_limit_kb")]
    transfer_limit_kb: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct PropagationAcknowledgeSyncParams {
    #[serde(default)]
    reset_state: bool,
    #[serde(default)]
    failure_state: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PropagationRemoteFetchParams {
    remote: String,
    #[serde(default)]
    identity_private_key_hex: Option<String>,
    #[serde(default)]
    timeout_secs: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_python_transfer_limit_kb")]
    transfer_limit_kb: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MessageDeliveryTraceParams {
    message_id: String,
}

#[derive(Debug, Deserialize)]
struct OutboundLxmQueryParams {
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    lxm_hash: Option<String>,
}

fn deserialize_python_transfer_limit_kb<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(value) = Option::<JsonValue>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let Some(limit) = transfer_limit_kb_from_json_value(&value) else {
        return Err(serde::de::Error::custom("invalid transfer_limit_kb"));
    };
    Ok(limit)
}

fn transfer_limit_kb_from_json_value(value: &JsonValue) -> Option<Option<f64>> {
    let limit = match value {
        JsonValue::Null => return Some(None),
        JsonValue::Number(value) => value.as_f64(),
        JsonValue::String(value) => value.trim().parse::<f64>().ok(),
        JsonValue::Bool(value) => Some(f64::from(*value as u8)),
        _ => None,
    }?;
    if limit.is_nan() {
        None
    } else if limit.is_infinite() && limit.is_sign_positive() {
        Some(None)
    } else {
        Some(Some(limit.max(0.0)))
    }
}
