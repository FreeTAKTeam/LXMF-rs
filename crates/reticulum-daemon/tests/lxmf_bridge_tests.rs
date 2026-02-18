use reticulum::identity::PrivateIdentity;
use reticulum_daemon::lxmf_bridge::{
    build_wire_message, decode_wire_message, json_to_rmpv, rmpv_to_json,
};

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

    let roundtrip = message.fields.and_then(|value| rmpv_to_json(&value));
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
fn json_to_rmpv_roundtrip() {
    let input = serde_json::json!({"arr": [1, true, "ok"], "n": 9});
    let value = json_to_rmpv(&input).expect("to rmpv");
    let output = rmpv_to_json(&value).expect("to json");
    assert_eq!(output, input);
}

#[test]
fn json_to_rmpv_preserves_noncanonical_numeric_keys() {
    let input = serde_json::json!({
        "1": "canonical-int",
        "01": "leading-zero",
        "+1": "plus-prefixed",
        "-01": "noncanonical-negative",
    });
    let value = json_to_rmpv(&input).expect("to rmpv");
    let output = rmpv_to_json(&value).expect("to json");
    assert_eq!(output, input);
}

#[test]
fn build_wire_message_normalizes_attachment_object_metadata() {
    let identity = PrivateIdentity::new_from_name("attachment-normalization");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x33u8; 16];

    let fields = serde_json::json!({
        "attachments": [
            {
                "name": "legacy.txt",
                "size": 3
            },
            {
                "name": "payload.bin",
                "data": [9, 8, 7]
            }
        ],
        "5": [
            {
                "filename": "override.bin",
                "data": [1, 2, 3]
            },
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value)).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["override.bin", [1, 2, 3]]]));
    assert!(fields.get("attachments").is_none());
}

#[test]
fn build_wire_message_normalizes_hex_and_base64_attachment_data() {
    let identity = PrivateIdentity::new_from_name("hex-base64-normalization");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x44u8; 16];

    let fields = serde_json::json!({
        "5": [
            {
                "filename": "hex.bin",
                "data": "0a0b0c",
            },
            {
                "name": "b64.bin",
                "data": "AQID",
            },
            {
                "name": "invalid.skipme",
                "data": "zz",
            },
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value)).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["hex.bin", [10, 11, 12]], ["b64.bin", [1, 2, 3]]]));
}

#[test]
fn build_wire_message_rejects_ambiguous_attachment_strings_without_prefix() {
    let identity = PrivateIdentity::new_from_name("ambiguous-string-normalization");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x45u8; 16];

    let fields = serde_json::json!({
        "5": [
            {
                "filename": "ambiguous.bin",
                "data": "deadbeef",
            },
            {
                "filename": "explicit-hex.bin",
                "data": "hex:deadbeef",
            },
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value)).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["explicit-hex.bin", [222, 173, 190, 239]]]));
}

#[test]
fn build_wire_message_skips_invalid_attachment_entries() {
    let identity = PrivateIdentity::new_from_name("invalid-entries");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x55u8; 16];

    let fields = serde_json::json!({
        "5": [
            ["good.bin", [1, 2, 3]],
            ["bad.decimal", -1],
            "bad-entry",
            {
                "name": "bad.hex",
                "data": "0a0b",
            },
            {
                "filename": "bad.string",
                "data": "not-bytes",
            },
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value)).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["good.bin", [1, 2, 3]]]));
}

#[test]
fn build_wire_message_uses_legacy_files_alias_when_field_5_invalid() {
    let identity = PrivateIdentity::new_from_name("legacy-files-alias");
    let mut source = [0u8; 16];
    source.copy_from_slice(identity.address_hash().as_slice());
    let destination = [0x66u8; 16];

    let fields = serde_json::json!({
        "5": [
            {
                "filename": "bad.hex",
                "data": "0a0b",
            },
            "bad-entry",
        ],
        "files": [
            ["good.bin", [1, 2, 3]],
        ],
    });

    let wire = build_wire_message(source, destination, "title", "content", Some(fields), &identity)
        .expect("wire");
    let message = decode_wire_message(&wire).expect("decode");

    let fields = message.fields.and_then(|value| rmpv_to_json(&value)).expect("fields");
    assert_eq!(fields["5"], serde_json::json!([["good.bin", [1, 2, 3]]]));
    assert!(fields.get("files").is_none());
}
