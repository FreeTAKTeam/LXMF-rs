use lxmf::constants::PN_META_NAME;
use lxmf::helpers::{
    display_name_from_app_data, pn_announce_data_is_valid, pn_name_from_app_data,
    pn_peering_cost_from_app_data, pn_stamp_cost_flexibility_from_app_data,
    pn_stamp_cost_from_app_data,
};
use rmpv::Value;
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
    assert_eq!(display_name_from_app_data(&app_data), Some("Alice".to_string()));
}

#[test]
fn display_name_from_app_data_parses_legacy_utf8() {
    assert_eq!(display_name_from_app_data(b"Bob Legacy"), Some("Bob Legacy".to_string()));
}

#[test]
fn pn_announce_data_allows_absent_metadata() {
    let payload = rmp_serde::to_vec(&(
        false,
        1_700_000_000u64,
        true,
        111u32,
        222u32,
        vec![20u32, 4u32, 25u32],
    ))
    .expect("partial announce data");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_name_from_app_data(&payload), None);
    assert_eq!(pn_stamp_cost_from_app_data(&payload), Some(20));
}

#[test]
fn pn_announce_data_parses_name_from_variant_keys() {
    let payload = rmp_serde::to_vec(&vec![
        Value::from(false),
        Value::from(1_700_000_000u64),
        Value::from(1u32),
        Value::from(111u32),
        Value::from(222u32),
        Value::from(vec![Value::from(20u32)]),
        Value::Map(vec![(Value::from("display_name"), Value::from("Variant Node"))]),
    ])
    .expect("announce data with display_name key");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_name_from_app_data(&payload), Some("Variant Node".to_string()));
    assert_eq!(pn_stamp_cost_from_app_data(&payload), Some(20));
}

#[test]
fn pn_announce_data_parses_costs_from_map_payload() {
    let payload = rmp_serde::to_vec(&vec![
        Value::from(false),
        Value::from(1_700_000_000u64),
        Value::from(true),
        Value::from(111u32),
        Value::from(222u32),
        Value::Map(vec![
            (Value::from("stamp_cost"), Value::from("20")),
            (Value::from("stamp_cost_flexibility"), Value::from(4u32)),
            (Value::from("peering_cost"), Value::from(25u32)),
        ]),
        Value::Map(vec![(Value::from("display_name"), Value::from("Map Node"))]),
    ])
    .expect("announce data with map costs");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_stamp_cost_from_app_data(&payload), Some(20));
    assert_eq!(pn_stamp_cost_flexibility_from_app_data(&payload), Some(4));
    assert_eq!(pn_peering_cost_from_app_data(&payload), Some(25));
}

#[test]
fn pn_announce_data_allows_float_stamp_cost() {
    let payload = rmp_serde::to_vec(&vec![
        Value::from(false),
        Value::from(1_700_000_000u64),
        Value::from(false),
        Value::from(111u32),
        Value::from(222u32),
        Value::from(vec![Value::from(20.0f64), Value::from(4u32), Value::from(25u32)]),
        Value::Map(vec![(Value::from(PN_META_NAME), Value::from("Floaty".as_bytes().to_vec()))]),
    ])
    .expect("announce data with float stamp cost");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_name_from_app_data(&payload), Some("Floaty".to_string()));
    assert_eq!(pn_stamp_cost_from_app_data(&payload), Some(20));
}

#[test]
fn pn_announce_data_allows_string_and_boolean_text_fields() {
    let payload = rmp_serde::to_vec(&vec![
        Value::from(false),
        Value::from("1700000000"),
        Value::from("true"),
        Value::from("111"),
        Value::from("222"),
        Value::from(vec![Value::from("20"), Value::from("4"), Value::from("25")]),
        Value::Map(vec![(Value::from("name"), Value::from("Stringy Node"))]),
    ])
    .expect("announce data with stringified fields");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_name_from_app_data(&payload), Some("Stringy Node".to_string()));
    assert_eq!(pn_stamp_cost_from_app_data(&payload), Some(20));
}

#[test]
fn pn_announce_data_parses_flexibility_and_peering_costs() {
    let payload = rmp_serde::to_vec(&vec![
        Value::from(false),
        Value::from(1_700_000_000u64),
        Value::from(true),
        Value::from(111u32),
        Value::from(222u32),
        Value::from(vec![Value::from(20u32), Value::from("4"), Value::from(9.0f64)]),
        Value::Map(vec![(Value::from("display_name"), Value::from("Flex Node"))]),
    ])
    .expect("announce data with flexibility and peering");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_stamp_cost_from_app_data(&payload), Some(20));
    assert_eq!(pn_stamp_cost_flexibility_from_app_data(&payload), Some(4));
    assert_eq!(pn_peering_cost_from_app_data(&payload), Some(9));
}

#[test]
fn pn_announce_data_returns_none_when_costs_missing_or_short() {
    let payload = rmp_serde::to_vec(&vec![
        Value::from(false),
        Value::from(1_700_000_000u64),
        Value::from(true),
        Value::from(111u32),
        Value::from(222u32),
        Value::from(vec![Value::from(20u32)]),
    ])
    .expect("short announce costs");

    assert!(pn_announce_data_is_valid(&payload));
    assert_eq!(pn_stamp_cost_flexibility_from_app_data(&payload), None);
    assert_eq!(pn_peering_cost_from_app_data(&payload), None);
}
