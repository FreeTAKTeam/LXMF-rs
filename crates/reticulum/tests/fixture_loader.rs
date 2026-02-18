#[test]
fn loads_fixture_bytes() {
    let bytes = std::fs::read("tests/fixtures/python/reticulum/packet_basic.bin").unwrap();
    assert!(!bytes.is_empty());
}
