use lxmf::WireMessage;
use reticulum_daemon::lxmf_bridge::{
    build_wire_message, build_wire_message_with_options,
    build_wire_message_with_options_and_cancel, decode_wire_message, json_to_rmpv, rmpv_to_json,
};
use reticulum_daemon::lxmf_stamps::ticket_stamp;
use rns_core::identity::PrivateIdentity;

#[test]
fn wire_roundtrip_preserves_content_title_fields() {
    let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let dest = [42u8; 16];
    let fields = serde_json::json!({"k": "v", "n": 2});

    let wire = build_wire_message(source, dest, "Hello", "World", Some(fields.clone()), &identity)
        .expect("wire");

    let message = decode_wire_message(&wire).expect("decode");
    assert_eq!(message.title_as_string().as_deref(), Some("Hello"));
    assert_eq!(message.content_as_string().as_deref(), Some("World"));

    let roundtrip = message.fields.and_then(|value| rmpv_to_json(&value).ok());
    assert_eq!(roundtrip, Some(fields));
}

#[test]
fn rmpv_to_json_decodes_columba_meta_from_string() {
    let fields = serde_json::json!({
        "112": r#"{"sender":"alpha","type":"columba"}"#
    });

    let value = json_to_rmpv(&fields).expect("to rmpv");
    let output = rmpv_to_json(&value).expect("to json");
    assert_eq!(output["112"], serde_json::json!({"sender":"alpha","type":"columba"}));
}

#[test]
fn rmpv_to_json_decodes_columba_meta_from_binary_json() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(112_i64.into()),
        rmpv::Value::Binary(br#"{"sender":"beta","type":"columba"}"#.to_vec()),
    )]);

    let output = rmpv_to_json(&fields).expect("to json");
    assert_eq!(output["112"], serde_json::json!({"sender":"beta","type":"columba"}));
}

#[test]
fn rmpv_to_json_decodes_columba_meta_from_binary_utf8_msgpack() {
    let packed = rmp_serde::to_vec(&rmpv::Value::Integer(77_i64.into())).expect("pack meta");
    let output = rmpv_to_json(&rmpv::Value::Map(vec![(
        rmpv::Value::Integer(112_i64.into()),
        rmpv::Value::Binary(packed),
    )]))
    .expect("to json");

    assert_eq!(output["112"], serde_json::json!(77));
}

#[test]
fn rmpv_to_json_decodes_telemetry_stream_from_string_payload() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::String("3".into()),
        rmpv::Value::String("\u{7f}".into()),
    )]);

    let output = rmpv_to_json(&fields).expect("to json");
    assert_eq!(output["3"], serde_json::json!(127));
}

#[test]
fn rmpv_to_json_preserves_nondecodable_telemetry_payload_as_generic() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(3_i64.into()),
        rmpv::Value::String("\u{0100}".into()),
    )]);

    // A field whose key ("3") matches the telemetry decoder but whose value is not a
    // decodable telemetry stream is not in that format; rather than rejecting the whole
    // message we fall back to the generic representation, preserving the value.
    let output = rmpv_to_json(&fields).expect("to json");
    assert_eq!(output["3"], serde_json::json!("\u{0100}"));
}

#[test]
fn rmpv_to_json_preserves_generic_value_sharing_client_field_key() {
    // Key "2" matches the Sideband-location decoder, but an integer value is plainly not
    // that format. It must fall back to generic conversion, not reject the whole message.
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(2_i64.into()),
        rmpv::Value::Integer(1.into()),
    )]);

    let output = rmpv_to_json(&fields).expect("to json");
    assert_eq!(output["2"], serde_json::json!(1));
}

#[test]
fn rmpv_to_json_preserves_invalid_columba_meta_from_binary() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(112_i64.into()),
        rmpv::Value::Binary(vec![0xc4]),
    )]);

    let output = rmpv_to_json(&fields).expect("to json");
    assert_eq!(output["112"], serde_json::json!([196]));
}

#[test]
fn json_to_rmpv_roundtrip() {
    let input = serde_json::json!({"arr": [1, true, "ok"], "n": 9});
    let value = json_to_rmpv(&input).expect("to rmpv");
    let output = rmpv_to_json(&value).expect("to json");
    assert_eq!(output, input);
}

#[test]
fn json_to_rmpv_preserves_noncanonical_numeric_keys_as_strings() {
    let input = serde_json::json!({
        "01": "leading-zero",
        "-01": "noncanonical-negative",
    });
    let value = json_to_rmpv(&input).expect("to rmpv");
    let output = rmpv_to_json(&value).expect("to json");
    assert_eq!(output["01"], serde_json::json!("leading-zero"));
    assert_eq!(output["-01"], serde_json::json!("noncanonical-negative"));
}

#[test]
fn build_wire_message_accepts_canonical_attachment_objects() {
    let identity = PrivateIdentity::new_from_name("attachment-normalization");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x33u8; 16];

    let fields = serde_json::json!({
        "attachments": [
            {
                "name": "legacy.txt",
                "size": 3,
                "data": [3]
            },
            {
                "name": "payload.bin",
                "data": [9, 8, 7]
            },
        ]
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value).ok()).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["legacy.txt", [3]], ["payload.bin", [9, 8, 7]]]));
    assert!(fields.get("attachments").is_none());
}

#[test]
fn build_wire_message_normalizes_hex_and_base64_attachment_data() {
    let identity = PrivateIdentity::new_from_name("hex-base64-normalization");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x44u8; 16];

    let fields = serde_json::json!({
        "attachments": [
            {
                "name": "hex.bin",
                "data": "hex:0a0b0c",
            },
            {
                "name": "b64.bin",
                "data": "base64:AQID",
            },
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value).ok()).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["hex.bin", [10, 11, 12]], ["b64.bin", [1, 2, 3]]]));
}

#[test]
fn build_wire_message_rejects_ambiguous_attachment_strings_without_prefix() {
    let identity = PrivateIdentity::new_from_name("ambiguous-string-normalization");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x45u8; 16];

    let fields = serde_json::json!({
        "attachments": [
            {
                "name": "ambiguous.bin",
                "data": "deadbeef",
            },
        ],
    });

    let err = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect_err("ambiguous attachment text must fail");
    assert!(err.to_string().contains("attachment text data must use explicit"));
}

#[test]
fn build_wire_message_with_include_ticket_adds_ticket_field() {
    let identity = PrivateIdentity::new_from_name("include-ticket");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x55u8; 16];
    let expires_at = 1_900_000_000_i64;
    let ticket = [0xabu8; 16];

    let wire = build_wire_message_with_options(
        source,
        destination,
        "title",
        "content",
        None,
        &identity,
        None,
        None,
        Some((expires_at, &ticket)),
    )
    .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");
    let fields = message.fields.expect("fields");
    let json = rmpv_to_json(&fields).expect("json");

    assert_eq!(json["12"][0].as_i64(), Some(expires_at));
    assert_eq!(
        json["12"][1],
        serde_json::json!([
            171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171, 171
        ])
    );
}

#[test]
fn build_wire_message_with_stamp_cost_generates_stamp() {
    let identity = PrivateIdentity::new_from_name("stamp-cost");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x66u8; 16];

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
    let message = decode_wire_message(&wire).expect("decode");

    assert_eq!(message.stamp.as_ref().map(Vec::len), Some(8));
}

#[test]
fn build_wire_message_with_stamp_cost_honors_cancellation() {
    let identity = PrivateIdentity::new_from_name("cancel-stamp-cost");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x67u8; 16];

    let err = build_wire_message_with_options_and_cancel(
        source,
        destination,
        "title",
        "content",
        None,
        &identity,
        Some(1),
        None,
        None,
        || true,
    )
    .expect_err("cancelled stamp generation should fail payload build");

    assert!(err.to_string().contains("failed to generate LXMF stamp"));
}

#[test]
fn build_wire_message_with_outbound_ticket_generates_ticket_stamp() {
    let identity = PrivateIdentity::new_from_name("outbound-ticket");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x77u8; 16];
    let ticket = [0xcdu8; 16];
    let ticket_hex = hex::encode(ticket);

    let wire = build_wire_message_with_options(
        source,
        destination,
        "title",
        "content",
        None,
        &identity,
        None,
        Some(&ticket_hex),
        None,
    )
    .expect("wire");
    let message = WireMessage::unpack(&wire).expect("decode wire");
    let expected_stamp = ticket_stamp(&ticket, &message.message_id());

    assert_eq!(
        message.payload.stamp.as_ref().map(|stamp| stamp.as_ref()),
        Some(expected_stamp.as_slice())
    );
}

#[test]
fn build_wire_message_rejects_invalid_attachment_entries() {
    let identity = PrivateIdentity::new_from_name("invalid-entries");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x55u8; 16];

    let fields = serde_json::json!({
        "attachments": [
            {
                "name": "good.bin",
                "data": [1, 2, 3],
            },
            "bad-entry",
        ],
    });

    let err = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect_err("invalid attachment entries must fail");
    assert!(err.to_string().contains("attachments must be objects with canonical shape"));
}

#[test]
fn build_wire_message_accepts_legacy_files_alias() {
    let identity = PrivateIdentity::new_from_name("legacy-files-alias");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x66u8; 16];

    let fields = serde_json::json!({
        "files": [
            {
                "name": "good.bin",
                "data": [1, 2, 3],
            }
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("legacy files alias should normalize");
    let message = decode_wire_message(&wire).expect("decode");
    let fields = message.fields.and_then(|value| rmpv_to_json(&value).ok()).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["good.bin", [1, 2, 3]]]));
}

#[test]
fn build_wire_message_accepts_public_numeric_attachment_key() {
    let identity = PrivateIdentity::new_from_name("numeric-attachment-key");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x67u8; 16];

    let fields = serde_json::json!({
        "5": [
            ["bad.bin", [1, 2, 3]]
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("raw numeric key should pass through");
    let message = decode_wire_message(&wire).expect("decode");
    let fields = message.fields.and_then(|value| rmpv_to_json(&value).ok()).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["bad.bin", [1, 2, 3]]]));
}

#[test]
fn build_wire_message_rejects_mixed_attachment_aliases() {
    let identity = PrivateIdentity::new_from_name("mixed-attachment-aliases");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x68u8; 16];

    let fields = serde_json::json!({
        "attachments": [{"name": "good.bin", "data": [1, 2, 3]}],
        "5": [["bad.bin", [4, 5, 6]]],
    });

    let err = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect_err("mixed aliases must fail");
    assert!(err.to_string().contains("attachment aliases"));
}
