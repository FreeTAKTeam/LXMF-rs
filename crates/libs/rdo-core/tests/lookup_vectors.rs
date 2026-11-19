//! Raw signed RNS announce lookup (feature `rns`), positive and negative.

mod common;

use common::*;
use rdo_core::lookup::verify_announce;

#[test]
fn every_binding_announce_resolves_to_its_owner() {
    let fixtures = fixtures();
    let owner = public(&fixtures, "owner_public_hex");
    for binding in fixtures["bindings"].as_array().expect("bindings") {
        let wanted: [u8; 16] = fixed(hex(&binding["destination_hex"]));
        let bound = verify_announce(&hex(&binding["announce_packet_hex"]), &wanted)
            .unwrap_or_else(|error| panic!("{}: {error}", binding["label"]));
        assert_eq!(bound.owner, owner);
        assert_eq!(bound.token.to_vec(), hex(&binding["token_hex"]));
    }
}

#[test]
fn negative_lookups_are_rejected() {
    let fixtures = fixtures();
    for case in fixtures["negative_lookup"].as_array().expect("negative lookups") {
        assert_eq!(case["expected"], "reject");
        let wanted: [u8; 16] = fixed(hex(&case["wanted_destination_hex"]));
        let raw = hex(&case["announce_packet_hex"]);
        assert!(verify_announce(&raw, &wanted).is_err(), "{} must be rejected", case["case"]);
    }
}
