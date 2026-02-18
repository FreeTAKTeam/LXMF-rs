use rand_core::OsRng;
use reticulum::destination::link::Link;
use reticulum::destination::link::LinkHandleResult;
use reticulum::destination::{DestinationDesc, DestinationName};
use reticulum::identity::PrivateIdentity;
use reticulum::packet::{DestinationType, PacketType};
use tokio::sync::broadcast;

#[test]
fn link_proof_packet_targets_link_destination() {
    let _sender = PrivateIdentity::new_from_rand(OsRng);
    let receiver = PrivateIdentity::new_from_rand(OsRng);

    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: DestinationName::new("lxmf", "delivery"),
    };

    let (event_tx, _) = broadcast::channel(16);
    let mut outbound = Link::new(destination, event_tx.clone());
    let request = outbound.request();

    let mut inbound =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, event_tx)
            .expect("input link");

    let proof = inbound.prove();

    assert_eq!(proof.header.destination_type, DestinationType::Link);
    assert_eq!(proof.destination, *inbound.id());
}

#[test]
fn link_packet_proof_includes_packet_hash() {
    let _sender = PrivateIdentity::new_from_rand(OsRng);
    let receiver = PrivateIdentity::new_from_rand(OsRng);

    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: DestinationName::new("lxmf", "delivery"),
    };

    let (event_tx, _) = broadcast::channel(16);
    let mut outbound = Link::new(destination, event_tx.clone());
    let request = outbound.request();

    let mut inbound =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, event_tx)
            .expect("input link");

    let data_packet = inbound.data_packet(&[1u8; 8]).expect("data packet");
    let expected_hash = data_packet.hash().to_bytes();

    let result = inbound.handle_packet(&data_packet);
    match result {
        LinkHandleResult::Proof(packet) => {
            assert_eq!(packet.header.destination_type, DestinationType::Link);
            assert_eq!(packet.header.packet_type, PacketType::Proof);
            assert!(packet.data.as_slice().starts_with(&expected_hash));
        }
        _ => panic!("expected proof packet"),
    }
}
