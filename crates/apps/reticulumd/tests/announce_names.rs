use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name, parse_peer_name_from_app_data,
};
use rmpv::Value;

#[test]
fn parse_peer_name_prefers_pn_metadata() {
    let app_data = encode_pn_announcement_app_data("Alice PN");
    let parsed = parse_peer_name_from_app_data(&app_data).expect("name from pn metadata");
    assert_eq!(parsed.0, "Alice PN");
    assert_eq!(parsed.1, "pn_meta");
}

fn encode_pn_announcement_app_data(name: &str) -> Vec<u8> {
    // pn_meta is parsed from the final map in the router announce app_data payload:
    // parse_peer_name_from_app_data reads key `1` from that map as `name`.
    rmp_serde::to_vec(&Value::Array(vec![
        Value::Boolean(false),
        Value::from(1_700_000_000_u64),
        Value::Boolean(true),
        Value::from(16_u32),
        Value::from(40_u32),
        Value::Array(vec![Value::from(16_u32), Value::from(16_u32), Value::from(18_u32)]),
        Value::Map(vec![(Value::from(1_u8), Value::Binary(name.as_bytes().to_vec()))]),
    ]))
    .expect("pn app data")
}

#[test]
fn parse_peer_name_falls_back_to_utf8_payload() {
    let parsed = parse_peer_name_from_app_data(b"  Bob UTF8  ").expect("name from utf8");
    assert_eq!(parsed.0, "Bob UTF8");
    assert_eq!(parsed.1, "app_data_utf8");
}

#[test]
fn parse_peer_name_reads_delivery_msgpack_app_data() {
    let app_data = rmp_serde::to_vec(&Value::Array(vec![
        Value::Binary(b"Alice Delivery".to_vec()),
        Value::from(9),
    ]))
    .expect("pack delivery app data");
    let parsed = parse_peer_name_from_app_data(&app_data).expect("name from delivery app data");
    assert_eq!(parsed.0, "Alice Delivery");
    assert_eq!(parsed.1, "delivery_app_data");
}

#[test]
fn parse_peer_name_rejects_binary_noise() {
    let app_data = [0xff, 0x00, 0xa5, 0x10, 0x80];
    assert!(parse_peer_name_from_app_data(&app_data).is_none());
}

#[test]
fn normalize_display_name_trims_and_caps_length() {
    let long = "x".repeat(200);
    let normalized = normalize_display_name(&long).expect("normalized");
    assert_eq!(normalized.chars().count(), 64);
}

#[test]
fn normalize_display_name_rejects_control_chars() {
    assert!(normalize_display_name("Alice\nBob").is_none());
    assert!(normalize_display_name("   ").is_none());
}

#[test]
fn encode_delivery_display_name_round_trips() {
    let app_data = encode_delivery_display_name_app_data("Alice Router").expect("encoded");
    let parsed = parse_peer_name_from_app_data(&app_data).expect("parsed");
    assert_eq!(parsed.0, "Alice Router");
    assert_eq!(parsed.1, "delivery_app_data");
}
