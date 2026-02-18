#[test]
fn verifies_python_signature() {
    let msg = b"hello";
    let sig = std::fs::read("tests/fixtures/python/reticulum/identity_sign.bin").unwrap();
    let pubkey_bytes =
        std::fs::read("tests/fixtures/python/reticulum/identity_pubkey.bin").unwrap();
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&pubkey_bytes);
    assert!(reticulum::identity::verify(pubkey, msg, &sig));
}

#[test]
fn private_identity_round_trips_fixture_bytes() {
    let fixture = std::fs::read("tests/fixtures/python/reticulum/identity.bin").unwrap();
    let identity = reticulum::identity::PrivateIdentity::from_private_key_bytes(&fixture).unwrap();
    assert_eq!(identity.to_private_key_bytes(), fixture.as_slice());
}
