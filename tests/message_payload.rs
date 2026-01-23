use lxmf::message::Payload;

#[test]
fn payload_roundtrip_msgpack() {
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let bytes = payload.to_msgpack().unwrap();
    let decoded = Payload::from_msgpack(&bytes).unwrap();
    assert_eq!(decoded.timestamp, payload.timestamp);
    assert_eq!(decoded.content, payload.content);
}
