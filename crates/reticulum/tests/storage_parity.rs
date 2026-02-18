#[test]
fn routing_table_serialization_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/python/reticulum/routing_table.bin").unwrap();
    let table = reticulum::transport::path_table::PathTable::new();
    let encoded = table.to_msgpack().unwrap();
    assert_eq!(encoded, fixture);
}
