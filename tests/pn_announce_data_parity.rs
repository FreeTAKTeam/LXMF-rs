use lxmf::helpers::{
    pn_announce_data_is_valid, pn_name_from_app_data, pn_stamp_cost_from_app_data,
};

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
