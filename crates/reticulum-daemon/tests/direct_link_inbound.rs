use lxmf::constants::DESTINATION_LENGTH;
use lxmf::inbound_decode::InboundPayloadMode;
use rand_core::OsRng;
use reticulum::identity::PrivateIdentity;
use reticulum_daemon::inbound_delivery::decode_inbound_payload;
use reticulum_daemon::lxmf_bridge::build_wire_message;

#[test]
fn inbound_link_payload_is_decoded() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let mut source = [0u8; 16];
    source.copy_from_slice(signer.address_hash().as_slice());
    let destination = source;

    let wire = build_wire_message(source, destination, "", "hello inbound", None, &signer)
        .expect("wire message");

    let payload = wire[DESTINATION_LENGTH..].to_vec();

    let record =
        decode_inbound_payload(destination, &payload, InboundPayloadMode::DestinationStripped)
            .expect("decoded record");

    assert_eq!(record.destination, hex::encode(destination));
    assert_eq!(record.content, "hello inbound");
    assert_eq!(record.direction, "in");
}
