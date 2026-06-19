use std::borrow::Cow;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use base64::Engine;

use lxmf::inbound_decode::{decode_inbound_message, InboundPayloadMode};

use lxmf::WireMessage;

use rns_rpc::{MessageRecord, RpcDaemon, RpcRequest};

use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::lxmf_bridge::rmpv_to_json;

use crate::lxmf_stamps::{invalid_stamp_value, validate_stamp};

pub fn decode_inbound_payload(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Option<MessageRecord> {
    decode_inbound_payload_with_diagnostics(destination, payload, mode).0
}

pub fn inbound_record_allowed_by_delivery_policy(
    daemon: &RpcDaemon,
    record: &MessageRecord,
) -> bool {
    let policy = daemon
        .handle_rpc(RpcRequest { id: 0, method: "get_delivery_policy".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.get("policy").cloned())
        .unwrap_or_else(|| json!({}));
    !policy.get("ignored_destinations").and_then(JsonValue::as_array).is_some_and(|entries| {
        entries
            .iter()
            .filter_map(JsonValue::as_str)
            .any(|entry| entry.eq_ignore_ascii_case(record.source.as_str()))
    })
}

#[derive(Debug, Clone)]
pub struct DecodeAttempt {
    pub candidate: &'static str,
    pub len: usize,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct InboundDecodeDiagnostics {
    pub attempts: Vec<DecodeAttempt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundStampStatus {
    pub checked: bool,
    pub valid: bool,
    pub value: Option<u32>,
}

impl InboundDecodeDiagnostics {
    pub fn summary(&self) -> String {
        if self.attempts.is_empty() {
            return "no decode attempts".to_string();
        }
        self.attempts
            .iter()
            .map(|attempt| format!("{}(len={}):{}", attempt.candidate, attempt.len, attempt.error))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

pub fn decode_inbound_payload_with_diagnostics(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> (Option<MessageRecord>, InboundDecodeDiagnostics) {
    let mut diagnostics = InboundDecodeDiagnostics::default();
    match decode_inbound_payload_mode(destination, payload, mode) {
        Ok(record) => (Some(record), diagnostics),
        Err(error) => {
            diagnostics.attempts.push(DecodeAttempt {
                candidate: inbound_mode_label(mode),
                len: payload.len(),
                error: error.to_string(),
            });
            (None, diagnostics)
        }
    }
}

fn decode_inbound_payload_mode(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<MessageRecord, lxmf::LxmfError> {
    let message = decode_inbound_message(destination, payload, mode)?;
    let fields =
        merge_inbound_lxmf_metadata(message.fields.as_ref().and_then(rmpv_to_json), &message);
    Ok(MessageRecord {
        id: message.id,
        source: hex::encode(message.source),
        destination: hex::encode(message.destination),
        title: String::from_utf8(message.title.clone()).unwrap_or_default(),
        content: String::from_utf8(message.content.clone()).unwrap_or_default(),
        timestamp: message.timestamp_f64 as i64,
        direction: "in".into(),
        fields,
        receipt_status: None,
    })
}

fn merge_inbound_lxmf_metadata(
    fields: Option<JsonValue>,
    message: &lxmf::inbound_decode::DecodedInboundMessage,
) -> Option<JsonValue> {
    let title_utf8 = decode_utf8_owned(message.title.clone(), "inbound title");
    let content_utf8 = decode_utf8_owned(message.content.clone(), "inbound content");
    let needs_metadata =
        title_utf8.is_none() || content_utf8.is_none() || message.timestamp_f64.fract() != 0.0;
    if !needs_metadata {
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
    let mut lxmf = match root.remove("_lxmf") {
        Some(JsonValue::Object(map)) => map,
        _ => JsonMap::new(),
    };
    lxmf.insert("timestamp_f64".into(), JsonValue::from(message.timestamp_f64));
    if title_utf8.is_none() {
        lxmf.insert(
            "title_base64".into(),
            JsonValue::String(BASE64_STANDARD.encode(&message.title)),
        );
    }
    if content_utf8.is_none() {
        lxmf.insert(
            "content_base64".into(),
            JsonValue::String(BASE64_STANDARD.encode(&message.content)),
        );
    }
    root.insert("_lxmf".into(), JsonValue::Object(lxmf));
    Some(JsonValue::Object(root))
}

pub fn annotate_inbound_record_stamp_status(
    record: &mut MessageRecord,
    stamp_status: InboundStampStatus,
) {
    if !stamp_status.checked {
        return;
    }

    let mut root = match record.fields.take() {
        Some(JsonValue::Object(map)) => map,
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("_fields_raw".into(), other);
            map
        }
        None => JsonMap::new(),
    };
    let mut lxmf = match root.remove("_lxmf") {
        Some(JsonValue::Object(map)) => map,
        _ => JsonMap::new(),
    };
    lxmf.insert("stamp_checked".into(), JsonValue::Bool(true));
    lxmf.insert("stamp_valid".into(), JsonValue::Bool(stamp_status.valid));
    if let Some(value) = stamp_status.value {
        lxmf.insert("stamp_value".into(), JsonValue::from(value));
    }
    root.insert("_lxmf".into(), JsonValue::Object(lxmf));
    record.fields = Some(JsonValue::Object(root));
}

pub fn inbound_stamp_policy_allows_payload(
    daemon: &RpcDaemon,
    fallback_destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<(), String> {
    evaluate_inbound_stamp_policy(daemon, fallback_destination, payload, mode).map(|_| ())
}

pub fn evaluate_inbound_stamp_policy(
    daemon: &RpcDaemon,
    fallback_destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<InboundStampStatus, String> {
    let policy = daemon.current_stamp_policy();
    if policy.target_cost == 0 {
        return Ok(InboundStampStatus { checked: false, valid: false, value: None });
    }

    let wire = match mode {
        InboundPayloadMode::FullWire => Cow::Borrowed(payload),
        InboundPayloadMode::DestinationStripped => {
            let mut with_destination_prefix = Vec::with_capacity(16 + payload.len());
            with_destination_prefix.extend_from_slice(&fallback_destination);
            with_destination_prefix.extend_from_slice(payload);
            Cow::Owned(with_destination_prefix)
        }
    };
    let message = WireMessage::unpack(wire.as_ref())
        .map_err(|error| format!("stamp validation decode failed: {error}"))?;
    let source_hex = hex::encode(message.source);
    let tickets = daemon.valid_issued_tickets_for(source_hex.as_str());
    let stamp = message.payload.stamp.as_deref().map(|value| value.as_ref());
    let accepted_cost = policy.target_cost.saturating_sub(policy.flexibility);
    if let Some(value) = validate_stamp(stamp, &message.message_id(), accepted_cost, &tickets) {
        return Ok(InboundStampStatus { checked: true, valid: true, value: Some(value) });
    }

    if !policy.enforce {
        return Ok(InboundStampStatus {
            checked: true,
            valid: false,
            value: invalid_stamp_value(stamp, &message.message_id()),
        });
    }

    Err(format!(
        "invalid LXMF stamp for source {} and target cost {}",
        source_hex, policy.target_cost
    ))
}

fn inbound_mode_label(mode: InboundPayloadMode) -> &'static str {
    match mode {
        InboundPayloadMode::FullWire => "full_wire",
        InboundPayloadMode::DestinationStripped => "destination_stripped",
    }
}

fn decode_utf8_owned(data: Vec<u8>, context: &str) -> Option<String> {
    match String::from_utf8(data) {
        Ok(text) => Some(text),
        Err(err) => {
            log::warn!("[daemon-rx] invalid UTF-8 in {context}: {err}");
            None
        }
    }
}
