use lxmf::router::Router;

const APP_DATA_TIMESTAMP: u64 = 1_700_000_000;

#[test]
fn propagation_node_app_data_custom_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/python/lxmf/propagation_node_app_data_custom.bin")
        .expect("propagation node app-data custom fixture");

    let mut router = Router::default();
    router.set_name("TestNode");
    router.set_propagation_node(true);
    router.set_propagation_limits(111, 222);
    router.set_propagation_stamp_cost(20, 4);
    router.set_peering_cost(25);

    let app_data = router
        .get_propagation_node_app_data_at(APP_DATA_TIMESTAMP)
        .expect("custom propagation node app-data");

    assert_eq!(app_data, fixture);
}
