#[test]
fn message_state_transitions() {
    let mut msg = lxmf::message::Message::new();
    msg.set_state(lxmf::message::State::Outbound);
    assert!(msg.is_outbound());
}
