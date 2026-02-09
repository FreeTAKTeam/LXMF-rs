use lxmf::message::{Payload, WireMessage};
use reticulum::identity::PrivateIdentity;

#[test]
fn payload_accepts_4_or_5_elements() {
    let payload = Payload::new(
        1.0,
        Some(b"hello".to_vec()),
        Some(b"title".to_vec()),
        None,
        None,
    );
    let encoded = payload.to_msgpack().expect("encode");
    let decoded = Payload::from_msgpack(&encoded).expect("decode");
    assert_eq!(decoded.timestamp, 1.0);
    assert!(decoded.stamp.is_none());

    let payload_with_stamp = Payload::new(
        2.0,
        Some(b"hello".to_vec()),
        Some(b"title".to_vec()),
        None,
        Some(vec![1u8; 16]),
    );
    let encoded = payload_with_stamp.to_msgpack().expect("encode");
    let decoded = Payload::from_msgpack(&encoded).expect("decode");
    assert_eq!(decoded.timestamp, 2.0);
    assert!(decoded.stamp.is_some());
}

#[test]
fn message_id_ignores_stamp() {
    let dest = [0x11u8; 16];
    let src = [0x22u8; 16];
    let payload = Payload::new(3.0, Some(vec![0x01]), None, None, None);
    let payload_with_stamp = Payload::new(3.0, Some(vec![0x01]), None, None, Some(vec![0x02; 16]));
    let wire = WireMessage::new(dest, src, payload);
    let wire_with_stamp = WireMessage::new(dest, src, payload_with_stamp);
    assert_eq!(wire.message_id(), wire_with_stamp.message_id());
}

#[test]
fn sign_verify_with_stamp_present() {
    let signer = PrivateIdentity::new_from_name("stamp-test");
    let dest = [0x33u8; 16];
    let mut src = [0u8; 16];
    src.copy_from_slice(signer.address_hash().as_slice());
    let payload = Payload::new(
        4.0,
        Some(b"content".to_vec()),
        None,
        None,
        Some(vec![0x55; 16]),
    );
    let mut wire = WireMessage::new(dest, src, payload);
    wire.sign(&signer).expect("sign");
    let packed = wire.pack().expect("pack");
    let decoded = WireMessage::unpack(&packed).expect("unpack");
    assert!(decoded.verify(signer.as_identity()).expect("verify"));
}
