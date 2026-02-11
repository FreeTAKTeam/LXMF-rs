use lxmf::propagation::unpack_envelope;

#[test]
fn propagation_envelope_unpack_matches_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/propagation.bin").unwrap();
    let envelope = unpack_envelope(&bytes).unwrap();

    assert_eq!(envelope.timestamp, 1_700_000_000.0);
    assert_eq!(envelope.messages.len(), 1);
    assert!(envelope.messages[0].len() > 16);
}
