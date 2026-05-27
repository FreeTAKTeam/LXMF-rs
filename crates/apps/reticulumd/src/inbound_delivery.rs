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
    let title_utf8 = String::from_utf8(message.title.clone()).ok();
    let content_utf8 = String::from_utf8(message.content.clone()).ok();
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
    if let Some(value) = validate_stamp(stamp, &message.message_id(), policy.target_cost, &tickets)
    {
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

#[cfg(test)]
mod tests {
    use super::{
        annotate_inbound_record_stamp_status, decode_inbound_payload_with_diagnostics,
        evaluate_inbound_stamp_policy, inbound_record_allowed_by_delivery_policy,
        inbound_stamp_policy_allows_payload, InboundStampStatus,
    };
    use lxmf::inbound_decode::InboundPayloadMode;
    use lxmf::{Payload, WireMessage};
    use rns_core::identity::PrivateIdentity;
    use rns_rpc::{RpcDaemon, RpcRequest};

    use crate::lxmf_bridge::build_wire_message_with_options;

    #[test]
    fn decode_inbound_payload_accepts_integer_timestamp_wire() {
        let destination = [0x11; 16];
        let source = [0x22; 16];
        let signature = [0x33; 64];
        let payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
            rmpv::Value::from(1_770_000_000_i64),
            rmpv::Value::from("title"),
            rmpv::Value::from("hello from python-like payload"),
            rmpv::Value::Nil,
        ]))
        .expect("payload encoding");
        let mut wire = Vec::new();
        wire.extend_from_slice(&destination);
        wire.extend_from_slice(&source);
        wire.extend_from_slice(&signature);
        wire.extend_from_slice(&payload);

        let (record, _) = decode_inbound_payload_with_diagnostics(
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        );
        let record = record.expect("decoded record");
        assert_eq!(record.source, hex::encode(source));
        assert_eq!(record.destination, hex::encode(destination));
        assert_eq!(record.title, "title");
        assert_eq!(record.content, "hello from python-like payload");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.direction, "in");
    }

    #[test]
    fn decode_inbound_payload_preserves_float_timestamp_and_binary_fields_in_metadata() {
        let identity = PrivateIdentity::new_from_name("inbound-fidelity-binary");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x73u8; 16];
        let payload = Payload::new(
            1_770_000_000.25,
            Some(b"body\0\xff".to_vec()),
            Some(b"\xfftitle".to_vec()),
            Some(rmpv::Value::Map(vec![(
                rmpv::Value::String("meta".into()),
                rmpv::Value::String("python-storage".into()),
            )])),
            None,
        );
        let mut wire = WireMessage::new(destination, source, payload);
        wire.sign(&identity).expect("sign");
        let packed = wire.pack().expect("pack");

        let (record, _) = decode_inbound_payload_with_diagnostics(
            destination,
            &packed,
            InboundPayloadMode::FullWire,
        );
        let record = record.expect("decoded record");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.title, "");
        assert_eq!(record.content, "");
        let fields = record.fields.expect("fields");
        assert_eq!(fields["meta"], serde_json::json!("python-storage"));
        assert_eq!(fields["_lxmf"]["timestamp_f64"], serde_json::json!(1_770_000_000.25));
        assert_eq!(fields["_lxmf"]["title_base64"], serde_json::json!("/3RpdGxl"));
        assert_eq!(fields["_lxmf"]["content_base64"], serde_json::json!("Ym9keQD/"));
    }

    #[test]
    fn inbound_stamp_policy_rejects_missing_stamp_when_required() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 4, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-missing-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x71u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            None,
            None,
        )
        .expect("wire");

        let err = inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect_err("missing stamp must be rejected");
        assert!(err.contains("invalid LXMF stamp"));
    }

    #[test]
    fn inbound_stamp_policy_reports_invalid_status_when_enforcement_disabled() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({
                    "target_cost": 4,
                    "flexibility": 0,
                    "enforce": false,
                })),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-invalid-stamp-observed");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x75u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            None,
            None,
        )
        .expect("wire");

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("invalid stamp should be observable when enforcement is disabled");

        assert!(status.checked);
        assert!(!status.valid);
        assert!(status.value.is_none());

        let mut record = decode_inbound_payload_with_diagnostics(
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .0
        .expect("decoded record");
        annotate_inbound_record_stamp_status(&mut record, status);
        let fields = record.fields.expect("fields");
        assert_eq!(fields["_lxmf"]["stamp_checked"], serde_json::json!(true));
        assert_eq!(fields["_lxmf"]["stamp_valid"], serde_json::json!(false));
        assert_eq!(fields["_lxmf"].get("stamp_value"), None);
    }

    #[test]
    fn inbound_stamp_status_annotation_sets_lxmf_flags() {
        let mut record = rns_rpc::MessageRecord {
            id: "msg-1".into(),
            source: "aa".into(),
            destination: "bb".into(),
            title: String::new(),
            content: String::new(),
            timestamp: 0,
            direction: "in".into(),
            fields: Some(serde_json::json!({"meta": 1})),
            receipt_status: None,
        };

        annotate_inbound_record_stamp_status(
            &mut record,
            InboundStampStatus { checked: true, valid: true, value: Some(17) },
        );
        let fields = record.fields.expect("fields");
        assert_eq!(fields["meta"], serde_json::json!(1));
        assert_eq!(fields["_lxmf"]["stamp_checked"], serde_json::json!(true));
        assert_eq!(fields["_lxmf"]["stamp_valid"], serde_json::json!(true));
        assert_eq!(fields["_lxmf"]["stamp_value"], serde_json::json!(17));
    }

    fn record_from_source(source: &str) -> rns_rpc::MessageRecord {
        rns_rpc::MessageRecord {
            id: "msg".to_string(),
            source: source.to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        }
    }

    #[test]
    fn inbound_delivery_policy_rejects_ignored_source_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "set_delivery_policy".to_string(),
                params: Some(serde_json::json!({
                    "ignored_destinations": ["aabbcc"],
                })),
            })
            .expect("set delivery policy");

        assert!(!inbound_record_allowed_by_delivery_policy(&daemon, &record_from_source("AABBCC")));
    }

    #[test]
    fn inbound_delivery_policy_allows_non_ignored_source() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "set_delivery_policy".to_string(),
                params: Some(serde_json::json!({
                    "ignored_destinations": ["aabbcc"],
                })),
            })
            .expect("set delivery policy");

        assert!(inbound_record_allowed_by_delivery_policy(&daemon, &record_from_source("ddeeff")));
    }

    #[test]
    fn inbound_stamp_policy_returns_checked_status_when_valid() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 20,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-status-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x79u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            Some(1),
            None,
            None,
        )
        .expect("wire");

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("valid stamp status");
        assert!(status.checked);
        assert!(status.valid);
        assert!(status.value.is_some_and(|value| value >= 1));
    }

    #[test]
    fn inbound_stamp_policy_accepts_generated_pow_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-pow-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x72u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            Some(1),
            None,
            None,
        )
        .expect("wire");

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("valid pow stamp should pass");
    }

    #[test]
    fn inbound_stamp_policy_accepts_issued_ticket_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 3,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 16, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-ticket-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let source_hex = hex::encode(source);
        let ticket = daemon.ensure_ticket(source_hex.as_str(), None).expect("issue ticket");
        let destination = [0x73u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            Some(ticket.ticket.as_str()),
            None,
        )
        .expect("wire");

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("ticket-validated stamp should pass");
    }

    #[test]
    fn inbound_stamp_policy_accepts_issued_ticket_stamp_above_ticket_cost_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 257, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-high-cost-ticket-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let source_hex = hex::encode(source);
        let ticket = daemon.ensure_ticket(source_hex.as_str(), None).expect("issue ticket");
        let destination = [0x74u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            Some(ticket.ticket.as_str()),
            None,
        )
        .expect("wire");

        let status = evaluate_inbound_stamp_policy(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("ticket-validated stamp should pass");
        assert!(status.checked);
        assert!(status.valid);
        assert_eq!(status.value, Some(crate::lxmf_stamps::COST_TICKET));
    }

    #[test]
    fn inbound_stamp_policy_accepts_destination_stripped_pow_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-destination-stripped-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x74u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            Some(1),
            None,
            None,
        )
        .expect("wire");
        let stripped = &wire[16..];

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            stripped,
            InboundPayloadMode::DestinationStripped,
        )
        .expect("valid destination-stripped stamp should pass");
    }
}
