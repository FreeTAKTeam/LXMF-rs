#[test]
fn message_constants_match_python_lengths() {
    assert_eq!(lxmf::constants::DESTINATION_LENGTH, 16);
    assert_eq!(lxmf::constants::SIGNATURE_LENGTH, 64);
    assert_eq!(lxmf::constants::TICKET_LENGTH, 16);
}
