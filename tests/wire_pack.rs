use lxmf::message::{Payload, WireMessage};
use reticulum::identity::PrivateIdentity;

#[test]
fn pack_unpack_roundtrip() {
    let payload = Payload::new(1_700_000_000.0, Some(b"hi".to_vec()), None, None, None);
    let mut msg = WireMessage::new([2u8; 16], [3u8; 16], payload);
    let signer = PrivateIdentity::new_from_name("lxmf-pack");
    msg.sign(&signer).unwrap();

    let bytes = msg.pack().unwrap();
    let decoded = WireMessage::unpack(&bytes).unwrap();
    assert_eq!(decoded.destination, msg.destination);
    assert_eq!(decoded.source, msg.source);
    assert!(decoded.signature.is_some());
}
