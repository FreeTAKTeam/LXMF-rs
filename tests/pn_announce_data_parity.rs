use lxmf::helpers::{
    display_name_from_app_data, pn_announce_data_is_valid, pn_name_from_app_data,
    pn_stamp_cost_from_app_data,
};
use serde_bytes::ByteBuf;

#[test]
fn pn_announce_data_parses_default_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/propagation_node_app_data.bin")
        .expect("default app data fixture");

    assert!(pn_announce_data_is_valid(&bytes));
    assert_eq!(pn_name_from_app_data(&bytes), None);
    assert_eq!(pn_stamp_cost_from_app_data(&bytes), Some(16));
}

#[test]
fn pn_announce_data_parses_custom_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/propagation_node_app_data_custom.bin")
        .expect("custom app data fixture");

    assert!(pn_announce_data_is_valid(&bytes));
    assert_eq!(pn_name_from_app_data(&bytes), Some("TestNode".to_string()));
    assert_eq!(pn_stamp_cost_from_app_data(&bytes), Some(20));
}

#[test]
fn display_name_from_delivery_app_data_parses_msgpack_list() {
    let app_data = rmp_serde::to_vec(&(Some(ByteBuf::from(b"Alice".to_vec())), Some(12u8)))
        .expect("pack peer app-data");
    assert_eq!(
        display_name_from_app_data(&app_data),
        Some("Alice".to_string())
    );
}

#[test]
fn display_name_from_app_data_parses_legacy_utf8() {
    assert_eq!(
        display_name_from_app_data(b"Bob Legacy"),
        Some("Bob Legacy".to_string())
    );
}
