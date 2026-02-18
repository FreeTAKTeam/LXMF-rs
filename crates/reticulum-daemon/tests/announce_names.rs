use lxmf::Router;
use reticulum_daemon::announce_names::{
    encode_delivery_display_name_app_data, normalize_display_name, parse_peer_name_from_app_data,
};
use rmpv::Value;

#[test]
fn parse_peer_name_prefers_pn_metadata() {
    let mut router = Router::default();
    router.set_name("Alice PN");
    let app_data = router.get_propagation_node_app_data().expect("pn app data");

    let parsed = parse_peer_name_from_app_data(&app_data).expect("name from pn metadata");
    assert_eq!(parsed.0, "Alice PN");
    assert_eq!(parsed.1, "pn_meta");
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
