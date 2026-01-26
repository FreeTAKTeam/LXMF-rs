#[test]
fn message_pack_bytes_match_fixture() {
    let fixture = std::fs::read("tests/fixtures/python/lxmf/message_packed.bin").unwrap();
    let msg = lxmf::message::Message::from_wire(&fixture).unwrap();
    let packed = msg.to_wire(None).unwrap();
    assert_eq!(packed, fixture);
}
