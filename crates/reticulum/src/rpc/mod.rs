pub mod codec;
mod daemon;
pub mod http;
use rmpv::Value as MsgPackValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::storage::messages::{AnnounceRecord, MessageRecord, MessagesStore};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::time::Duration;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcResponse {
    pub id: u64,
    pub result: Option<JsonValue>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct InterfaceRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct DeliveryPolicy {
    pub auth_required: bool,
    pub allowed_destinations: Vec<String>,
    pub denied_destinations: Vec<String>,
    pub ignored_destinations: Vec<String>,
    pub prioritised_destinations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct PropagationState {
    pub enabled: bool,
    pub store_root: Option<String>,
    pub target_cost: u32,
    pub total_ingested: usize,
    pub last_ingest_count: usize,
    pub sync_state: u32,
    pub state_name: String,
    pub sync_progress: f64,
    pub messages_received: usize,
    pub max_messages: usize,
    pub selected_node: Option<String>,
    pub last_sync_started: Option<i64>,
    pub last_sync_completed: Option<i64>,
    pub last_sync_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct StampPolicy {
    pub target_cost: u32,
    pub flexibility: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TicketRecord {
    pub destination: String,
    pub ticket: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DeliveryTraceEntry {
    pub status: String,
    pub timestamp: i64,
    #[serde(default)]
    pub reason_code: Option<String>,
}

pub struct RpcDaemon {
    store: MessagesStore,
    identity_hash: String,
    delivery_destination_hash: Mutex<Option<String>>,
    events: broadcast::Sender<RpcEvent>,
    event_queue: Mutex<VecDeque<RpcEvent>>,
    peers: Mutex<HashMap<String, PeerRecord>>,
    interfaces: Mutex<Vec<InterfaceRecord>>,
    delivery_policy: Mutex<DeliveryPolicy>,
    propagation_state: Mutex<PropagationState>,
    propagation_payloads: Mutex<HashMap<String, String>>,
    outbound_propagation_node: Mutex<Option<String>>,
    paper_ingest_seen: Mutex<HashSet<String>>,
    stamp_policy: Mutex<StampPolicy>,
    ticket_cache: Mutex<HashMap<String, TicketRecord>>,
    delivery_traces: Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>,
    outbound_bridge: Option<Arc<dyn OutboundBridge>>,
    announce_bridge: Option<Arc<dyn AnnounceBridge>>,
}

pub trait OutboundBridge: Send + Sync {
    fn deliver(
        &self,
        record: &MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error>;
}

pub trait AnnounceBridge: Send + Sync {
    fn announce_now(&self) -> Result<(), std::io::Error>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct OutboundDeliveryOptions {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub stamp_cost: Option<u32>,
    #[serde(default)]
    pub include_ticket: bool,
    #[serde(default)]
    pub try_propagation_on_fail: bool,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub source_private_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RpcEvent {
    pub event_type: String,
    pub payload: JsonValue,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PeerRecord {
    pub peer: String,
    pub last_seen: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_source: Option<String>,
    #[serde(default)]
    pub first_seen: i64,
    #[serde(default)]
    pub seen_count: u64,
}

#[derive(Debug, Deserialize)]
struct SendMessageParams {
    id: String,
    source: String,
    destination: String,
    #[serde(default)]
    title: String,
    content: String,
    fields: Option<JsonValue>,
    #[serde(default)]
    source_private_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageV2Params {
    id: String,
    source: String,
    destination: String,
    #[serde(default)]
    title: String,
    content: String,
    fields: Option<JsonValue>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    stamp_cost: Option<u32>,
    #[serde(default)]
    include_ticket: Option<bool>,
    #[serde(default)]
    try_propagation_on_fail: Option<bool>,
    #[serde(default)]
    source_private_key: Option<String>,
}

#[derive(Debug)]
struct NormalizedSendRequest {
    id: String,
    source: String,
    destination: String,
    title: String,
    content: String,
    fields: Option<JsonValue>,
    method: Option<String>,
    stamp_cost: Option<u32>,
    options: OutboundDeliveryOptions,
    include_ticket: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RecordReceiptParams {
    message_id: String,
    status: String,
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

fn parse_outbound_send_request(
    method: &str,
    params: JsonValue,
) -> Result<NormalizedSendRequest, std::io::Error> {
    match method {
        "send_message" => {
            let parsed: SendMessageParams = serde_json::from_value(params)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
            validate_outbound_fields_strict(parsed.fields.as_ref())?;
            let options = OutboundDeliveryOptions {
                source_private_key: parsed.source_private_key,
                ..Default::default()
            };
            Ok(NormalizedSendRequest {
                id: parsed.id,
                source: parsed.source,
                destination: parsed.destination,
                title: parsed.title,
                content: parsed.content,
                fields: parsed.fields,
                method: None,
                stamp_cost: None,
                options,
                include_ticket: None,
            })
        }
        "send_message_v2" => {
            let parsed: SendMessageV2Params = serde_json::from_value(params)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
            validate_outbound_fields_strict(parsed.fields.as_ref())?;
            let outbound_method = parsed.method.clone();
            let include_ticket = parsed.include_ticket;
            Ok(NormalizedSendRequest {
                id: parsed.id,
                source: parsed.source,
                destination: parsed.destination,
                title: parsed.title,
                content: parsed.content,
                fields: parsed.fields,
                method: outbound_method.clone(),
                stamp_cost: parsed.stamp_cost,
                options: OutboundDeliveryOptions {
                    method: outbound_method,
                    stamp_cost: parsed.stamp_cost,
                    include_ticket: include_ticket.unwrap_or_default(),
                    try_propagation_on_fail: parsed.try_propagation_on_fail.unwrap_or_default(),
                    ticket: None,
                    source_private_key: parsed.source_private_key,
                },
                include_ticket,
            })
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unsupported send method '{method}'"),
        )),
    }
}

fn validate_outbound_fields_strict(fields: Option<&JsonValue>) -> Result<(), std::io::Error> {
    let Some(JsonValue::Object(map)) = fields else {
        return Ok(());
    };

    if map.contains_key("files") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "legacy field 'files' is not allowed; use 'attachments'",
        ));
    }

    if map.contains_key("5") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "public field '5' is not allowed; use 'attachments'",
        ));
    }

    let Some(attachments) = map.get("attachments") else {
        return Ok(());
    };

    let attachments = attachments.as_array().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "field 'attachments' must be an array of attachment objects",
        )
    })?;

    for entry in attachments {
        validate_attachment_entry(entry)?;
    }

    Ok(())
}

fn validate_attachment_entry(entry: &JsonValue) -> Result<(), std::io::Error> {
    let Some(map) = entry.as_object() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "attachments must be objects with canonical shape",
        ));
    };

    let name = map.get("name").and_then(JsonValue::as_str).map(str::trim).unwrap_or_default();
    if name.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "attachment entry must include string field 'name'",
        ));
    }

    let Some(data) = map.get("data") else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "attachment entry must include field 'data'",
        ));
    };
    validate_attachment_data(data)
}

fn validate_attachment_data(value: &JsonValue) -> Result<(), std::io::Error> {
    match value {
        JsonValue::Array(items) => {
            for item in items {
                let valid = item
                    .as_u64()
                    .map(|value| value <= u8::MAX as u64)
                    .or_else(|| item.as_i64().map(|value| u8::try_from(value).is_ok()))
                    .unwrap_or(false);
                if !valid {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "attachment data array must contain bytes between 0 and 255",
                    ));
                }
            }
            Ok(())
        }
        JsonValue::String(text) => validate_attachment_text_data(text),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "attachment data must be an array of bytes or prefixed text data",
        )),
    }
}

fn validate_attachment_text_data(text: &str) -> Result<(), std::io::Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "attachment data string cannot be empty",
        ));
    }

    if let Some(payload) = text.strip_prefix("hex:").or_else(|| text.strip_prefix("HEX:")) {
        let payload = payload.trim();
        if payload.is_empty()
            || payload.len() % 2 != 0
            || !payload.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid hex attachment data after hex: prefix",
            ));
        }
        return Ok(());
    }

    if text.strip_prefix("base64:").or_else(|| text.strip_prefix("BASE64:")).is_some() {
        return Ok(());
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "attachment text data must use explicit 'hex:' or 'base64:' prefix",
    ))
}

fn merge_fields_with_options(
    fields: Option<JsonValue>,
    method: Option<String>,
    stamp_cost: Option<u32>,
    include_ticket: Option<bool>,
) -> Option<JsonValue> {
    let has_options = method.is_some() || stamp_cost.is_some() || include_ticket.is_some();
    if !has_options {
        return fields;
    }

    let mut root = match fields {
        Some(JsonValue::Object(map)) => map,
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("_fields_raw".into(), other);
            map
        }
        None => JsonMap::new(),
    };

    let mut lxmf = JsonMap::new();
    if let Some(value) = method {
        lxmf.insert("method".into(), JsonValue::String(value));
    }
    if let Some(value) = stamp_cost {
        lxmf.insert("stamp_cost".into(), json!(value));
    }
    if let Some(value) = include_ticket {
        lxmf.insert("include_ticket".into(), json!(value));
    }

    root.insert("_lxmf".into(), JsonValue::Object(lxmf));
    Some(JsonValue::Object(root))
}

fn now_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

fn first_n_chars(input: &str, n: usize) -> Option<String> {
    if n == 0 {
        return Some(String::new());
    }
    let end = input.char_indices().nth(n - 1).map(|(idx, ch)| idx + ch.len_utf8())?;
    Some(input[..end].to_string())
}

fn clean_optional_text(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn normalize_capabilities(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        out.push(normalized);
    }
    out
}

fn parse_capabilities_from_app_data_hex(app_data_hex: Option<&str>) -> Vec<String> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let Ok(app_data) = hex::decode(raw_hex) else {
        return Vec::new();
    };
    if app_data.is_empty() {
        return Vec::new();
    }

    let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) else {
        return Vec::new();
    };
    let mut capabilities = Vec::new();
    if let Some(entries) = value.as_array() {
        if entries.len() >= 3 && parse_bool_capability_flag(&entries[2]) {
            capabilities.push("propagation".to_string());
        }
        for entry in entries {
            if let Some(parsed) = extract_capabilities_from_msgpack(entry) {
                capabilities.extend(parsed);
            }
        }
    } else if let Some(parsed) = extract_capabilities_from_msgpack(&value) {
        capabilities.extend(parsed);
    }

    normalize_capabilities(capabilities)
}

fn parse_bool_capability_flag(value: &MsgPackValue) -> bool {
    match value {
        MsgPackValue::Boolean(true) => true,
        MsgPackValue::Integer(value) => value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
            .is_some_and(|value| value == 1),
        MsgPackValue::F64(value) => *value == 1.0,
        MsgPackValue::F32(value) => f64::from(*value) == 1.0,
        MsgPackValue::Binary(text) => parse_fuzzy_bool(std::str::from_utf8(text).ok()),
        MsgPackValue::String(text) => parse_fuzzy_bool(text.as_str()),
        _ => false,
    }
}

fn parse_fuzzy_bool(text: Option<&str>) -> bool {
    match text.map(str::trim).map(str::to_lowercase).as_deref() {
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        _ => false,
    }
}

fn parse_text_to_u32(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = trimmed.parse::<u32>() {
        return Some(value);
    }

    parse_f64_to_u32(trimmed.parse::<f64>().ok()?)
}

fn parse_f64_to_u32(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }

    if value > u32::MAX as f64 {
        return None;
    }

    Some(value as u32)
}

fn parse_fuzzy_u32(value: &MsgPackValue) -> Option<u32> {
    match value {
        MsgPackValue::Integer(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
            .or_else(|| value.as_f64().and_then(parse_f64_to_u32)),
        MsgPackValue::F64(value) => parse_f64_to_u32(*value),
        MsgPackValue::F32(value) => parse_f64_to_u32(f64::from(*value)),
        MsgPackValue::Boolean(value) => Some(u32::from(*value)),
        MsgPackValue::Binary(bytes) => parse_text_to_u32(std::str::from_utf8(bytes).ok()?),
        MsgPackValue::String(text) => parse_text_to_u32(text.as_str()?),
        _ => None,
    }
}

fn parse_announce_costs_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> (Option<u32>, Option<u32>) {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return (None, None);
    };
    let Ok(app_data) = hex::decode(raw_hex) else {
        return (None, None);
    };
    let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) else {
        return (None, None);
    };
    let Some(entries) = value.as_array() else {
        return (None, None);
    };
    let Some(costs) = entries.get(5) else {
        return (None, None);
    };
    if let MsgPackValue::Array(values) = costs {
        return (values.get(1).and_then(parse_fuzzy_u32), values.get(2).and_then(parse_fuzzy_u32));
    }
    let MsgPackValue::Map(entries) = costs else {
        return (None, None);
    };
    let mut stamp_cost_flexibility = None;
    let mut peering_cost = None;
    for (key, value) in entries {
        let Some(key) = msgpack_key_to_string(key) else {
            continue;
        };
        if key == "stamp_cost_flexibility" {
            stamp_cost_flexibility = parse_fuzzy_u32(value);
        }
        if key == "peering_cost" {
            peering_cost = parse_fuzzy_u32(value);
        }
    }
    (stamp_cost_flexibility, peering_cost)
}

fn extract_capabilities_from_msgpack(value: &MsgPackValue) -> Option<Vec<String>> {
    if let MsgPackValue::Array(entries) = value {
        return Some(normalize_capabilities(
            entries.iter().filter_map(capability_value_to_string).collect(),
        ));
    }

    let MsgPackValue::Map(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(key, value)| {
        if is_capability_key(key) {
            return extract_capabilities_from_msgpack(value);
        }
        None
    })
}

fn is_capability_key(key: &MsgPackValue) -> bool {
    msgpack_key_to_string(key).is_some_and(|name| matches!(name.as_str(), "caps" | "capabilities"))
}

fn capability_value_to_string(value: &MsgPackValue) -> Option<String> {
    match value {
        MsgPackValue::String(text) => text.as_str().map(str::to_string),
        MsgPackValue::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

fn msgpack_key_to_string(key: &MsgPackValue) -> Option<String> {
    match key {
        MsgPackValue::String(key) => key.as_str().map(|key| key.trim().to_ascii_lowercase()),
        MsgPackValue::Binary(key) => {
            String::from_utf8(key.clone()).ok().map(|key| key.trim().to_ascii_lowercase())
        }
        _ => None,
    }
}

fn encode_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn handle_framed_request(daemon: &RpcDaemon, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    daemon.handle_framed_request(bytes)
}
