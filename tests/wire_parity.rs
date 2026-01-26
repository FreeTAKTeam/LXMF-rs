#[test]
fn wire_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/wire_basic.bin").unwrap();
    let msg = lxmf::message::WireMessage::unpack(&bytes).unwrap();
    let encoded = msg.pack().unwrap();
    assert_eq!(bytes, encoded);
}
