use lxmf::message::Payload;
use rmpv::Value;

#[test]
fn payload_roundtrip_msgpack() {
    let payload = Payload::new(1_700_000_000.0, Some(b"hi".to_vec()), None, None, None);
    let bytes = payload.to_msgpack().unwrap();
    let decoded = Payload::from_msgpack(&bytes).unwrap();
    assert_eq!(decoded.timestamp, payload.timestamp);
    assert_eq!(decoded.content, payload.content);
}

#[test]
fn payload_decode_rejects_invalid_title_type() {
    let raw = Value::Array(vec![
        Value::F64(1.0),
        Value::from(123),
        Value::Nil,
        Value::Nil,
    ]);
    let bytes = rmp_serde::to_vec(&raw).unwrap();
    let err = Payload::from_msgpack(&bytes).unwrap_err();
    assert!(err.to_string().contains("invalid payload title"));
}

#[test]
fn payload_decode_rejects_invalid_stamp_type() {
    let raw = Value::Array(vec![
        Value::F64(1.0),
        Value::Nil,
        Value::Nil,
        Value::Nil,
        Value::Map(vec![]),
    ]);
    let bytes = rmp_serde::to_vec(&raw).unwrap();
    let err = Payload::from_msgpack(&bytes).unwrap_err();
    assert!(err.to_string().contains("invalid payload stamp"));
}
