use lxmf::message::{Payload, WireMessage};

#[test]
fn pack_unpack_roundtrip() {
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let msg = WireMessage::new([2u8; 16], [3u8; 16], payload);
    let bytes = msg.pack().unwrap();
    let decoded = WireMessage::unpack(&bytes).unwrap();
    assert_eq!(decoded.destination, msg.destination);
    assert_eq!(decoded.source, msg.source);
}
