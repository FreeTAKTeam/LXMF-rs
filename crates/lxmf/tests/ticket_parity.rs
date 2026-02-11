use serde::Deserialize;

#[derive(Deserialize)]
struct TicketCase {
    expires: f64,
    ticket: Vec<u8>,
    now: f64,
}

#[test]
fn ticket_validates_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/ticket_valid.msgpack").unwrap();
    let case: TicketCase = rmp_serde::from_slice(&bytes).unwrap();

    let ticket = lxmf::ticket::Ticket::new(case.expires, case.ticket);
    assert!(ticket.is_valid(case.now));
}

#[test]
fn ticket_rejects_expired_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/ticket_expired.msgpack").unwrap();
    let case: TicketCase = rmp_serde::from_slice(&bytes).unwrap();

    let ticket = lxmf::ticket::Ticket::new(case.expires, case.ticket);
    assert!(!ticket.is_valid(case.now));
}
