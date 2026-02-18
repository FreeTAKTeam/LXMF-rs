#[test]
fn decodes_python_packet() {
    let bytes = std::fs::read("tests/fixtures/python/reticulum/packet_basic.bin").unwrap();
    let packet = reticulum::packet::Packet::from_bytes(&bytes).unwrap();
    let encoded = packet.to_bytes().unwrap();
    assert_eq!(bytes, encoded);
}

#[test]
fn packet_header_matches_fixture() {
    use rand_core::OsRng;

    let fixture = std::fs::read("tests/fixtures/python/reticulum/packet_header.bin").unwrap();
    let identity_bytes = std::fs::read("tests/fixtures/python/reticulum/identity.bin").unwrap();
    let identity =
        reticulum::identity::PrivateIdentity::from_private_key_bytes(&identity_bytes).unwrap();
    let mut destination = reticulum::destination::new_in(identity, "lxmf", "delivery");
    let packet = destination.announce(OsRng, None).unwrap();
    let bytes = packet.to_bytes().unwrap();
    let header_len = 2 + reticulum::hash::ADDRESS_HASH_SIZE + 1;
    assert_eq!(&bytes[..header_len], fixture.as_slice());
}
