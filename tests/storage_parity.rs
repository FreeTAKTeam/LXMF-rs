#[test]
fn storage_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/storage_basic.bin").unwrap();
    let msg = lxmf::message::WireMessage::unpack_storage(&bytes).unwrap();
    let encoded = msg.pack_storage().unwrap();
    assert_eq!(bytes, encoded);
}
