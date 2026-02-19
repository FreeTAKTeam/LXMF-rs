use super::super::{
    build_propagation_envelope, build_wire_message, format_relay_request_status,
    normalize_relay_destination_hash, parse_alternative_relay_request_status,
    propagation_relay_candidates, PeerCrypto,
};
use crate::propagation::unpack_envelope;
use reticulum::identity::PrivateIdentity;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[test]
fn normalize_relay_destination_hash_preserves_destination_hash_input() {
    let destination_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
    let signer = PrivateIdentity::new_from_name("relay-preserve");
    let identity = *signer.as_identity();
    let mut peer_map = HashMap::new();
    peer_map.insert(destination_hash.clone(), PeerCrypto { identity });
    let peer_crypto = Arc::new(Mutex::new(peer_map));

    let resolved = normalize_relay_destination_hash(&peer_crypto, &destination_hash)
        .expect("should preserve known destination hash");
    assert_eq!(resolved, destination_hash);
}

#[test]
fn normalize_relay_destination_hash_maps_identity_hash_to_destination_hash() {
    let destination_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
    let signer = PrivateIdentity::new_from_name("relay-normalize");
    let identity = *signer.as_identity();
    let identity_hash = hex::encode(identity.address_hash.as_slice());
    let mut peer_map = HashMap::new();
    peer_map.insert(destination_hash.clone(), PeerCrypto { identity });
    let peer_crypto = Arc::new(Mutex::new(peer_map));

    let resolved = normalize_relay_destination_hash(&peer_crypto, &identity_hash)
        .expect("should map known identity hash to destination hash");
    assert_eq!(resolved, destination_hash);
}

#[test]
fn propagation_relay_candidates_prefer_selected_then_known_nodes() {
    let selected = Arc::new(Mutex::new(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string())));
    let known_nodes = Arc::new(Mutex::new(HashSet::from([
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        "cccccccccccccccccccccccccccccccc".to_string(),
    ])));

    let candidates = propagation_relay_candidates(&selected, &known_nodes);
    assert_eq!(candidates[0], "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert!(candidates.contains(&"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()));
    assert!(candidates.contains(&"cccccccccccccccccccccccccccccccc".to_string()));
    assert_eq!(candidates.len(), 3);
}

#[test]
fn relay_request_status_roundtrips_exclusions() {
    let status = format_relay_request_status(&[
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
    ]);
    let excludes =
        parse_alternative_relay_request_status(status.as_str()).expect("relay request status");
    assert_eq!(
        excludes,
        vec![
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()
        ]
    );
}

#[test]
fn build_propagation_envelope_wraps_wire_payload() {
    let signer = PrivateIdentity::new_from_name("propagation-envelope-signer");
    let recipient = PrivateIdentity::new_from_name("propagation-envelope-recipient");
    let mut source = [0u8; 16];
    source.copy_from_slice(signer.address_hash().as_slice());
    let destination = [0x44; 16];
    let wire =
        build_wire_message(source, destination, "", "hello", None, &signer).expect("wire payload");

    let envelope =
        build_propagation_envelope(&wire, recipient.as_identity()).expect("propagation envelope");
    let unpacked = unpack_envelope(&envelope).expect("decode propagation envelope");
    assert_eq!(unpacked.messages.len(), 1);
    assert_eq!(&unpacked.messages[0][..16], destination.as_slice());
    assert_ne!(unpacked.messages[0], wire);
}
