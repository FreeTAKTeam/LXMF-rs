#[test]
fn parity_matrix_has_no_missing_router_items() {
    let text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    assert!(!text.contains("missing") || !text.contains("router"));
}

#[test]
fn parity_matrix_marks_stamp_ticket_progress() {
    let text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    assert!(text.contains("LXMF/LXStamper.py") && text.contains("ticket"));
}
