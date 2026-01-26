#[test]
fn validates_python_stamp() {
    let data = std::fs::read("tests/fixtures/python/lxmf/stamp_basic.bin").unwrap();
    assert!(lxmf::stamper::stamp_valid(&data));
}
