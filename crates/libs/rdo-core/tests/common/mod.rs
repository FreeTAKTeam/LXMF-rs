#![allow(dead_code)]

use rdo_core::{PublicIdentity, SigningIdentity};
use serde_json::Value as Json;

pub fn fixtures() -> Json {
    serde_json::from_str(include_str!("../fixtures/owner-v1.json")).expect("fixture JSON")
}

pub fn hex(value: &Json) -> Vec<u8> {
    hex::decode(value.as_str().expect("hex string")).expect("valid hex")
}

pub fn fixed<const N: usize>(bytes: Vec<u8>) -> [u8; N] {
    bytes.try_into().expect("fixed-length fixture bytes")
}

pub fn owner(fixtures: &Json) -> SigningIdentity {
    SigningIdentity::from_private_bytes(&fixed(hex(&fixtures["fixture_identity_private_hex"])))
}

pub fn public(fixtures: &Json, key: &str) -> PublicIdentity {
    PublicIdentity(fixed(hex(&fixtures[key])))
}
