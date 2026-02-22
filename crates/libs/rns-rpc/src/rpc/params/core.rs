#[derive(Debug, Deserialize)]
struct RecordReceiptParams {
    message_id: String,
    status: String,
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
    stamp_cost_flexibility: Option<u32>,
    #[serde(default)]
    peering_cost: Option<u32>,
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
struct PropagationEnableParams {
    enabled: bool,
    #[serde(default)]
    store_root: Option<String>,
    #[serde(default)]
    target_cost: Option<u32>,
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
}

#[derive(Debug, Deserialize)]
struct TicketGenerateParams {
    destination: String,
    #[serde(default)]
    ttl_secs: Option<u64>,
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
    #[serde(default)]
    peer: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageDeliveryTraceParams {
    message_id: String,
}
