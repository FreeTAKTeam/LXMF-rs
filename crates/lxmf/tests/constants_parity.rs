#[test]
fn renderer_constants_match() {
    assert_eq!(lxmf::constants::RENDERER_PLAIN, 0x00);
    assert_eq!(lxmf::constants::FIELD_TICKET, 0x0C);
}
