use lxmf::message::{Payload, WireMessage};

#[test]
fn wire_message_id_is_stable() {
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let msg = WireMessage::new([0u8; 16], [1u8; 16], payload);
    let id1 = msg.message_id();
    let id2 = msg.message_id();
    assert_eq!(id1, id2);
}
