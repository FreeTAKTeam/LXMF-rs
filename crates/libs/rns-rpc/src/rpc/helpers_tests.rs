use super::*;

#[test]
fn merge_fields_with_options_preserves_existing_lxmf_metadata() {
    let fields = json!({
        "app": "value",
        "_lxmf": {
            "existing": true,
            "method": "direct"
        },
    });

    let merged = merge_fields_with_options(
        Some(fields),
        Some("propagated".to_string()),
        Some(7),
        Some(true),
    )
    .expect("merged fields");

    assert_eq!(merged["app"], json!("value"));
    assert_eq!(merged["_lxmf"]["existing"], json!(true));
    assert_eq!(merged["_lxmf"]["method"], json!("propagated"));
    assert_eq!(merged["_lxmf"]["stamp_cost"], json!(7));
    assert_eq!(merged["_lxmf"]["include_ticket"], json!(true));
}

#[test]
fn merge_fields_with_options_preserves_non_object_lxmf_metadata() {
    let fields = json!({
        "_lxmf": "legacy-marker",
    });

    let merged =
        merge_fields_with_options(Some(fields), None, Some(3), None).expect("merged fields");

    assert_eq!(merged["_lxmf"]["_raw"], json!("legacy-marker"));
    assert_eq!(merged["_lxmf"]["stamp_cost"], json!(3));
}

#[test]
fn outbound_wire_fields_strip_private_metadata_and_preserve_lxmf_numeric_fields() {
    let fields = json!({
        "title": "HIL Poco",
        "content": "host-to-poco",
        "body": "host-to-poco",
        "payload": { "raw": true },
        "_sdk": { "correlation_id": "corr-1" },
        "_lxmf": { "method": "direct" },
        "_fields_raw": "legacy",
        "9": [{ "command_type": "status.request" }],
        "12": [170, 187],
    });

    let wire = outbound_wire_fields(Some(fields)).expect("wire fields");

    assert_eq!(wire["9"][0]["command_type"], json!("status.request"));
    assert_eq!(wire["12"], json!([170, 187]));
    assert_eq!(wire.get("title"), None);
    assert_eq!(wire.get("content"), None);
    assert_eq!(wire.get("body"), None);
    assert_eq!(wire.get("payload"), None);
    assert_eq!(wire.get("_sdk"), None);
    assert_eq!(wire.get("_lxmf"), None);
    assert_eq!(wire.get("_fields_raw"), None);
}

#[test]
fn outbound_wire_fields_returns_none_when_only_private_metadata_remains() {
    let fields = json!({
        "title": "HIL Poco",
        "content": "host-to-poco",
        "_sdk": { "correlation_id": "corr-1" },
        "_lxmf": { "method": "direct" },
    });

    assert_eq!(outbound_wire_fields(Some(fields)), None);
}

#[test]
fn parses_capabilities_from_utf8_json_app_data() {
    let hex = hex::encode(r#"{"capabilities":["propagation","telemetry_relay"]}"#);
    let capabilities = parse_capabilities_from_app_data_hex(Some(hex.as_str()));
    assert_eq!(capabilities, vec!["propagation".to_string(), "telemetry_relay".to_string()]);
}

#[test]
fn parses_capabilities_from_tagged_utf8_text_app_data() {
    let hex = hex::encode("node metadata; caps=propagation, telemetry_relay");
    let capabilities = parse_capabilities_from_app_data_hex(Some(hex.as_str()));
    assert_eq!(capabilities, vec!["propagation".to_string(), "telemetry_relay".to_string()]);
}

#[test]
fn parses_rch_capabilities_from_msgpack_third_slot() {
    let capability_payload = rmp_serde::to_vec_named(&serde_json::json!({
        "app": "rch",
        "schema": 1,
        "caps": ["telemetry_relay", "topic_broker"],
    }))
    .expect("encode capability payload");
    let announce = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::String("Reticulum Community Hub".into()),
        rmpv::Value::from(0),
        rmpv::Value::Binary(capability_payload),
    ]))
    .expect("encode announce payload");
    let capabilities = parse_capabilities_from_app_data_hex(Some(hex::encode(announce).as_str()));
    assert_eq!(capabilities, vec!["telemetry_relay".to_string(), "topic_broker".to_string()]);
}

#[test]
fn parses_rch_capabilities_from_cbor_third_slot() {
    let mut capability_payload = Vec::new();
    ciborium::ser::into_writer(&serde_json::json!({
        "app": "rch",
        "schema": 1,
        "caps": ["telemetry_relay", "tak_bridge"],
    }), &mut capability_payload)
    .expect("encode cbor capability payload");
    let announce = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::String("Reticulum Community Hub".into()),
        rmpv::Value::from(0),
        rmpv::Value::Binary(capability_payload),
    ]))
    .expect("encode announce payload");
    let capabilities = parse_capabilities_from_app_data_hex(Some(hex::encode(announce).as_str()));
    assert_eq!(capabilities, vec!["telemetry_relay".to_string(), "tak_bridge".to_string()]);
}

#[test]
fn parses_delivery_stamp_cost_from_python_peer_data_slot() {
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Binary(b"Peer Name".to_vec()),
        MsgPackValue::from(23),
    ]))
    .expect("encode app data");

    assert_eq!(
        parse_delivery_stamp_cost_from_app_data_hex(Some(hex::encode(app_data).as_str())),
        Some(23)
    );
}

#[test]
fn rejects_python_invalid_delivery_stamp_costs_from_peer_data() {
    for invalid_cost in [0, 255, 256] {
        let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
            MsgPackValue::Binary(b"Peer Name".to_vec()),
            MsgPackValue::from(invalid_cost),
        ]))
        .expect("encode app data");

        assert_eq!(
            parse_delivery_stamp_cost_from_app_data_hex(Some(hex::encode(app_data).as_str())),
            None
        );
    }
}
