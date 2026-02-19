use super::super::{json_to_rmpv, rmpv_to_json};

#[test]
fn rmpv_to_json_decodes_sideband_packed_location_sensor() {
    let packed = rmp_serde::to_vec(&rmpv::Value::Map(vec![
        (rmpv::Value::Integer(1_i64.into()), rmpv::Value::Integer(1_770_855_315_i64.into())),
        (
            rmpv::Value::Integer(2_i64.into()),
            rmpv::Value::Array(vec![
                rmpv::Value::Binary((48_856_600_i32).to_be_bytes().to_vec()),
                rmpv::Value::Binary((2_352_200_i32).to_be_bytes().to_vec()),
                rmpv::Value::Binary((3550_i32).to_be_bytes().to_vec()),
                rmpv::Value::Binary((420_u32).to_be_bytes().to_vec()),
                rmpv::Value::Binary((18_000_i32).to_be_bytes().to_vec()),
                rmpv::Value::Binary((340_u16).to_be_bytes().to_vec()),
                rmpv::Value::Integer(1_770_855_315_i64.into()),
            ]),
        ),
    ]))
    .expect("pack telemetry");

    let fields =
        rmpv::Value::Map(vec![(rmpv::Value::Integer(2_i64.into()), rmpv::Value::Binary(packed))]);
    let decoded = rmpv_to_json(&fields).expect("decoded");

    assert_eq!(decoded["2"]["lat"], serde_json::json!(48.8566));
    assert_eq!(decoded["2"]["lon"], serde_json::json!(2.3522));
    assert_eq!(decoded["2"]["accuracy"], serde_json::json!(3.4));
    assert_eq!(decoded["2"]["updated"], serde_json::json!(1_770_855_315_i64));
}

#[test]
fn rmpv_to_json_decodes_columba_meta_from_string() {
    let fields = rmpv::Value::Map(vec![
        (
            rmpv::Value::Integer(112_i64.into()),
            rmpv::Value::String(r#"{"sender":"alpha","type":"columba"}"#.into()),
        ),
        (
            rmpv::Value::Integer(113_i64.into()),
            rmpv::Value::String("fallback-text".to_string().into()),
        ),
    ]);
    let decoded = rmpv_to_json(&fields).expect("decoded");

    assert_eq!(decoded["112"]["sender"], serde_json::json!("alpha"));
    assert_eq!(decoded["112"]["type"], serde_json::json!("columba"));
    assert_eq!(decoded["113"], serde_json::json!("fallback-text"));
}

#[test]
fn rmpv_to_json_decodes_columba_meta_from_binary_json() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(112_i64.into()),
        rmpv::Value::Binary(br#"{"sender":"beta","type":"columba"}"#.to_vec()),
    )]);
    let decoded = rmpv_to_json(&fields).expect("decoded");

    assert_eq!(decoded["112"]["sender"], serde_json::json!("beta"));
    assert_eq!(decoded["112"]["type"], serde_json::json!("columba"));
}

#[test]
fn rmpv_to_json_decodes_columba_meta_from_binary_utf8_msgpack() {
    let packed = rmp_serde::to_vec(&rmpv::Value::Integer(77_i64.into())).expect("pack meta");
    let fields =
        rmpv::Value::Map(vec![(rmpv::Value::Integer(112_i64.into()), rmpv::Value::Binary(packed))]);
    let decoded = rmpv_to_json(&fields).expect("decoded");

    assert_eq!(decoded["112"], serde_json::json!(77));
}

#[test]
fn rmpv_to_json_preserves_unparseable_columba_meta_from_binary() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(112_i64.into()),
        rmpv::Value::Binary(vec![0xc4]),
    )]);
    let decoded = rmpv_to_json(&fields).expect("decoded");

    assert_eq!(decoded["112"], serde_json::json!([196]));
}

#[test]
fn rmpv_to_json_decodes_telemetry_stream_from_string_payload() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(3_i64.into()),
        rmpv::Value::String("\u{7f}".into()),
    )]);

    let decoded = rmpv_to_json(&fields).expect("decoded");
    assert_eq!(decoded["3"], serde_json::json!(127));
}

#[test]
fn rmpv_to_json_preserves_nonbinary_telemetry_payload_as_string() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::Integer(2_i64.into()),
        rmpv::Value::String("\u{0100}".into()),
    )]);

    let decoded = rmpv_to_json(&fields).expect("decoded");
    assert_eq!(decoded["2"], serde_json::json!("\u{0100}"));
}

#[test]
fn rmpv_to_json_preserves_unparseable_telemetry_from_string_payload() {
    let fields = rmpv::Value::Map(vec![(
        rmpv::Value::String("3".into()),
        rmpv::Value::String("\u{0100}".into()),
    )]);

    let decoded = rmpv_to_json(&fields).expect("decoded");
    assert_eq!(decoded["3"], serde_json::json!("\u{0100}"));
}

#[test]
fn json_to_rmpv_preserves_noncanonical_numeric_keys_as_strings() {
    let fields = serde_json::json!({
        "01": "leading-zero",
        "-01": "noncanonical-negative",
    });
    let converted = json_to_rmpv(&fields).expect("to rmpv");
    let decoded = rmpv_to_json(&converted).expect("decoded");

    assert_eq!(decoded["01"], serde_json::json!("leading-zero"));
    assert_eq!(decoded["-01"], serde_json::json!("noncanonical-negative"));
}
