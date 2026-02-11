#[test]
fn payload_bytes_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/payload_bytes.bin").unwrap();
    let payload = lxmf::message::Payload::from_msgpack(&bytes).unwrap();
    let encoded = payload.to_msgpack().unwrap();
    assert_eq!(bytes, encoded);
}

#[test]
fn payload_strings_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/payload_strings.bin").unwrap();
    let payload = lxmf::message::Payload::from_msgpack(&bytes).unwrap();
    let encoded = payload.to_msgpack().unwrap();
    assert_eq!(bytes, encoded);
}
