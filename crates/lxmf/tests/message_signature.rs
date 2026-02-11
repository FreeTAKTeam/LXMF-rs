use lxmf::message::{Payload, WireMessage};
use reticulum::identity::PrivateIdentity;

#[test]
fn wire_message_sign_and_verify() {
    let payload = Payload::new(1_700_000_000.0, Some(b"hi".to_vec()), None, None, None);
    let mut msg = WireMessage::new([4u8; 16], [5u8; 16], payload);

    let signer = PrivateIdentity::new_from_name("lxmf-signer");
    msg.sign(&signer).unwrap();

    let identity = signer.as_identity();
    assert!(msg.verify(identity).unwrap());
}

#[test]
fn pack_includes_signature() {
    let payload = Payload::new(1_700_000_001.0, Some(b"yo".to_vec()), None, None, None);
    let mut msg = WireMessage::new([9u8; 16], [8u8; 16], payload);

    let signer = PrivateIdentity::new_from_name("lxmf-pack");
    msg.sign(&signer).unwrap();

    let bytes = msg.pack().unwrap();
    let decoded = WireMessage::unpack(&bytes).unwrap();
    assert!(decoded.signature.is_some());
}
