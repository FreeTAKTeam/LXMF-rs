#[test]
fn loads_lxmf_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/wire_basic.bin").unwrap();
    assert!(!bytes.is_empty());
}
