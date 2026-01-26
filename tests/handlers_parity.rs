use lxmf::error::LxmfError;

#[test]
fn delivery_announce_handler_invoked() {
    let mut handler = lxmf::handlers::DeliveryAnnounceHandler::new();
    assert!(handler.handle(&[0u8; 16]).is_ok());
}

#[test]
fn propagation_announce_handler_invoked() {
    let mut handler = lxmf::handlers::PropagationAnnounceHandler::new();
    let result: Result<(), LxmfError> = handler.handle(&[1u8; 16]);
    assert!(result.is_ok());
}
