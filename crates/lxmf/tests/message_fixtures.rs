#[test]
fn loads_lxmessage_fixtures() {
    let payload_bytes = std::fs::read("tests/fixtures/python/lxmf/payload_bytes.bin").unwrap();
    let payload_strings = std::fs::read("tests/fixtures/python/lxmf/payload_strings.bin").unwrap();
    let wire_signed = std::fs::read("tests/fixtures/python/lxmf/wire_signed.bin").unwrap();
    let message_packed = std::fs::read("tests/fixtures/python/lxmf/message_packed.bin").unwrap();
    let storage_unsigned =
        std::fs::read("tests/fixtures/python/lxmf/storage_unsigned.bin").unwrap();
    let storage_signed = std::fs::read("tests/fixtures/python/lxmf/storage_signed.bin").unwrap();
    let propagation = std::fs::read("tests/fixtures/python/lxmf/propagation.bin").unwrap();
    let paper = std::fs::read("tests/fixtures/python/lxmf/paper.bin").unwrap();
    let delivery_matrix =
        std::fs::read("tests/fixtures/python/lxmf/delivery_matrix.msgpack").unwrap();

    assert!(!payload_bytes.is_empty());
    assert!(!payload_strings.is_empty());
    assert!(!wire_signed.is_empty());
    assert!(!message_packed.is_empty());
    assert!(!storage_unsigned.is_empty());
    assert!(!storage_signed.is_empty());
    assert!(!propagation.is_empty());
    assert!(!paper.is_empty());
    assert!(!delivery_matrix.is_empty());
}
