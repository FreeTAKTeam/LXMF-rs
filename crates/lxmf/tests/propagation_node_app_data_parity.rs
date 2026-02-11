use lxmf::router::Router;

const APP_DATA_TIMESTAMP: u64 = 1_700_000_000;

#[test]
fn propagation_node_app_data_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/python/lxmf/propagation_node_app_data.bin")
        .expect("propagation node app-data fixture");

    let router = Router::default();
    let app_data = router.get_propagation_node_app_data_at(APP_DATA_TIMESTAMP);

    assert_eq!(app_data, fixture);
}
