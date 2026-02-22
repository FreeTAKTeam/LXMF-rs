use super::super::{
    build_send_params_with_source, build_wire_message, can_send_opportunistic,
    decode_inbound_payload, rmpv_to_json, sanitize_outbound_wire_fields, InboundPayloadMode,
    SendMessageRequest,
};
use crate::constants::FIELD_COMMANDS;
use crate::message::Message;
use crate::payload_fields::{CommandEntry, WireFields};
use reticulum::identity::PrivateIdentity;
use serde_json::{json, Value};

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

    let record = decode_inbound_payload(destination, &wire, InboundPayloadMode::FullWire)
        .expect("decoded record");
    assert_eq!(record.source, hex::encode(source));
    assert_eq!(record.destination, hex::encode(destination));
    assert_eq!(record.title, "title");
    assert_eq!(record.content, "hello from python-like payload");
    assert_eq!(record.timestamp, 1_770_000_000_i64);
    assert_eq!(record.direction, "in");
}

#[test]
fn build_wire_message_prefers_transport_msgpack_fields() {
    let mut fields = WireFields::new();
    fields.set_commands(vec![CommandEntry::from_text(0x01, "ping")]);
    let json_fields = fields.to_transport_json().expect("transport fields");

    let signer = PrivateIdentity::new_from_name("wire-fields-test");
    let source = [0x10; 16];
    let destination = [0x20; 16];
    let wire =
        build_wire_message(source, destination, "title", "content", Some(json_fields), &signer)
            .expect("wire");

    let decoded = Message::from_wire(&wire).expect("decode");
    let Some(rmpv::Value::Map(entries)) = decoded.fields else {
        panic!("fields should decode to map")
    };
    let commands = entries
        .iter()
        .find_map(|(key, value)| (key.as_i64() == Some(FIELD_COMMANDS as i64)).then_some(value))
        .expect("commands field");
    let rmpv::Value::Array(commands_list) = commands else { panic!("commands should be an array") };
    assert_eq!(commands_list.len(), 1);
}

#[test]
fn build_send_params_includes_expected_rpc_keys() {
    let request = SendMessageRequest {
        id: Some("msg-123".to_string()),
        source: Some("ignored".to_string()),
        source_private_key: Some(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff".to_string(),
        ),
        destination: "ffeeddccbbaa99887766554433221100".to_string(),
        title: "subject".to_string(),
        content: "body".to_string(),
        fields: Some(serde_json::json!({ "k": "v" })),
        method: Some("direct".to_string()),
        stamp_cost: Some(7),
        include_ticket: true,
        try_propagation_on_fail: true,
    };

    let prepared =
        build_send_params_with_source(request, "00112233445566778899aabbccddeeff".to_string())
            .expect("prepared");
    assert_eq!(prepared.id, "msg-123");
    assert_eq!(prepared.source, "00112233445566778899aabbccddeeff");
    assert_eq!(prepared.destination, "ffeeddccbbaa99887766554433221100");
    assert_eq!(prepared.params["method"], Value::String("direct".to_string()));
    assert_eq!(prepared.params["stamp_cost"], Value::from(7));
    assert_eq!(prepared.params["include_ticket"], Value::Bool(true));
    assert_eq!(prepared.params["try_propagation_on_fail"], Value::Bool(true));
    assert_eq!(
        prepared.params["source_private_key"],
        Value::String(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff".to_string()
        )
    );
    assert_eq!(prepared.params["fields"]["k"], Value::String("v".to_string()));
}

#[test]
fn sanitize_outbound_wire_fields_removes_transport_controls() {
    let fields = json!({
        "__delivery_options": {
            "method": "propagated",
            "stamp_cost": 128,
            "include_ticket": true
        },
        "_lxmf": {
            "method": "direct",
            "scope": "chat",
            "app": "weft",
        },
        "lxmf": {
            "try_propagation_on_fail": true,
            "app": "bridge",
        },
        "attachments": [],
    });
    let sanitized = sanitize_outbound_wire_fields(Some(&fields)).expect("sanitized");
    assert!(sanitized.get("__delivery_options").is_none());

    let Some(sanitized_lxmf) = sanitized.get("_lxmf").and_then(Value::as_object) else {
        panic!("_lxmf preserved")
    };
    assert!(sanitized_lxmf.get("method").is_none());
    assert_eq!(sanitized_lxmf.get("scope"), Some(&Value::String("chat".to_string())));
    assert_eq!(sanitized_lxmf.get("app"), Some(&Value::String("weft".to_string())));

    let Some(sanitized_alt_lxmf) = sanitized.get("lxmf").and_then(Value::as_object) else {
        panic!("lxmf preserved")
    };
    assert!(sanitized_alt_lxmf.get("try_propagation_on_fail").is_none());
    assert_eq!(sanitized_alt_lxmf.get("app"), Some(&Value::String("bridge".to_string())));
    assert_eq!(sanitized.get("attachments"), Some(&Value::Array(vec![])));
}

#[test]
fn sanitize_outbound_wire_fields_preserves_canonical_attachments() {
    let fields = json!({
        "attachments": [
            {
                "name": "sideband_note.txt",
                "data": [110, 111, 116, 101],
                "media_type": "text/plain",
            },
            {
                "name": "legacy.json",
                "data": [123, 125]
            }
        ],
    });
    let sanitized = sanitize_outbound_wire_fields(Some(&fields)).expect("sanitized");
    assert_eq!(sanitized.get("attachments"), fields.get("attachments"));
}

#[test]
fn build_wire_message_rejects_ambiguous_attachment_text_data() {
    let signer = PrivateIdentity::new_from_name("runtime-ambiguous-attachment");
    let source = [0x10; 16];
    let destination = [0x20; 16];
    let fields = json!({
        "attachments": [
            {
                "name": "ambiguous.bin",
                "data": "deadbeef",
            },
        ],
    });
    let err = build_wire_message(source, destination, "title", "content", Some(fields), &signer)
        .expect_err("ambiguous attachment text must fail");
    assert!(err.to_string().contains("attachment text data must use explicit"));
}

#[test]
fn build_wire_message_accepts_prefixed_attachment_data() {
    let signer = PrivateIdentity::new_from_name("runtime-prefixed-attachment");
    let source = [0x30; 16];
    let destination = [0x40; 16];
    let fields = json!({
        "attachments": [
            {
                "name": "hex.bin",
                "data": "hex:0a0b0c",
            },
            {
                "name": "b64.bin",
                "data": "base64:AQID",
            },
        ]
    });
    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &signer)
        .expect("wire");
    let decoded = Message::from_wire(&wire).expect("decode");
    let parsed = decoded.fields.as_ref().and_then(rmpv_to_json).expect("fields");
    assert_eq!(parsed.get("5"), Some(&json!([["hex.bin", [10, 11, 12]], ["b64.bin", [1, 2, 3]]])));
}

#[test]
fn build_wire_message_rejects_invalid_attachment_entries() {
    let signer = PrivateIdentity::new_from_name("runtime-invalid-attachment-entry");
    let source = [0x50; 16];
    let destination = [0x60; 16];
    let fields = json!({
        "attachments": [
            "bad-entry"
        ],
    });
    let err = build_wire_message(source, destination, "title", "content", Some(fields), &signer)
        .expect_err("invalid attachment entries must fail");
    assert!(err.to_string().contains("attachments must be objects with canonical shape"));
}

#[test]
fn build_wire_message_rejects_legacy_attachment_aliases() {
    let signer = PrivateIdentity::new_from_name("runtime-legacy-aliases");
    let source = [0x70; 16];
    let destination = [0x80; 16];
    let err = build_wire_message(
        source,
        destination,
        "title",
        "content",
        Some(json!({
            "files": [
                {
                    "name": "bad.bin",
                    "data": [1, 2, 3]
                }
            ]
        })),
        &signer,
    )
    .expect_err("legacy files alias must fail");
    assert!(err.to_string().contains("legacy field 'files' is not allowed"));

    let err = build_wire_message(
        source,
        destination,
        "title",
        "content",
        Some(json!({
            "5": [
                ["bad.bin", [1, 2, 3]]
            ]
        })),
        &signer,
    )
    .expect_err("public field 5 must fail");
    assert!(err.to_string().contains("public field '5' is not allowed"));
}

#[test]
fn fields_contain_attachments_from_sideband_metadata() {
    let fields = json!({
        "attachments": [
            {
                "name": "legacy.txt",
                "size": 3
            }
        ]
    });
    assert!(!can_send_opportunistic(Some(&fields), 1));
}

#[test]
fn build_send_params_rejects_empty_destination() {
    let request = SendMessageRequest {
        destination: "   ".to_string(),
        content: "body".to_string(),
        ..SendMessageRequest::default()
    };
    let err = build_send_params_with_source(request, "source".to_string()).expect_err("err");
    assert!(err.to_string().contains("destination is required"));
}
