#[test]
fn loads_stamp_ticket_fixtures() {
    let stamp_valid = std::fs::read("tests/fixtures/python/lxmf/stamp_valid.msgpack").unwrap();
    let stamp_invalid = std::fs::read("tests/fixtures/python/lxmf/stamp_invalid.msgpack").unwrap();
    let pn_stamp = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let ticket_valid = std::fs::read("tests/fixtures/python/lxmf/ticket_valid.msgpack").unwrap();
    let ticket_expired =
        std::fs::read("tests/fixtures/python/lxmf/ticket_expired.msgpack").unwrap();

    assert!(!stamp_valid.is_empty());
    assert!(!stamp_invalid.is_empty());
    assert!(!pn_stamp.is_empty());
    assert!(!ticket_valid.is_empty());
    assert!(!ticket_expired.is_empty());
}
