#[test]
fn storage_container_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/storage_unsigned.bin").unwrap();
    let container = lxmf::message::MessageContainer::from_msgpack(&bytes).unwrap();
    let encoded = container.to_msgpack().unwrap();
    assert_eq!(bytes, encoded);
}

#[test]
fn storage_container_with_state_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/storage_signed.bin").unwrap();
    let container = lxmf::message::MessageContainer::from_msgpack(&bytes).unwrap();
    let encoded = container.to_msgpack().unwrap();
    assert_eq!(bytes, encoded);
}
