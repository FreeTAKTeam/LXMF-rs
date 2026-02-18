use rand_core::OsRng;

#[test]
fn destination_hash_matches_python() {
    let identity = reticulum::identity::PrivateIdentity::new_from_rand(OsRng);
    let dest = reticulum::destination::new_in(identity, "app", "aspect");
    assert_eq!(dest.desc.address_hash.len(), 16);
}

#[test]
fn destination_hash_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/python/reticulum/destination_hash.bin").unwrap();
    let identity_bytes = std::fs::read("tests/fixtures/python/reticulum/identity.bin").unwrap();
    let identity =
        reticulum::identity::PrivateIdentity::from_private_key_bytes(&identity_bytes).unwrap();
    let dest = reticulum::destination::new_in(identity, "lxmf", "delivery");
    assert_eq!(dest.desc.address_hash.as_slice(), fixture.as_slice());
}
