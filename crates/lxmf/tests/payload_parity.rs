#[test]
fn payload_matches_python_msgpack() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/payload_basic.bin").unwrap();
    let payload = lxmf::message::Payload::from_msgpack(&bytes).unwrap();
    let encoded = payload.to_msgpack().unwrap();
    assert_eq!(bytes, encoded);
}
