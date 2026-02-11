use lxmf::message::{MessageContainer, MessageState, TransportMethod};

#[test]
fn storage_container_fields_match_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/storage_unsigned.bin").unwrap();
    let container = MessageContainer::from_msgpack(&bytes).unwrap();

    assert_eq!(container.state_enum().unwrap(), MessageState::Generating);
    assert_eq!(container.method_enum().unwrap(), TransportMethod::Direct);
    assert!(!container.transport_encrypted);
    assert!(container.transport_encryption.is_none());
}

#[test]
fn storage_container_state_and_method_match_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/storage_signed.bin").unwrap();
    let container = MessageContainer::from_msgpack(&bytes).unwrap();

    assert_eq!(container.state_enum().unwrap(), MessageState::Delivered);
    assert_eq!(container.method_enum().unwrap(), TransportMethod::Direct);
    assert!(container.transport_encrypted);
    assert_eq!(container.transport_encryption.as_deref(), Some("AES-128"));
}
