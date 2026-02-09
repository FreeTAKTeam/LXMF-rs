use lxmf::constants::{TICKET_LENGTH, WORKBLOCK_EXPAND_ROUNDS};
use lxmf::stamper::{cancel_work, generate_stamp, stamp_valid, stamp_workblock};
use lxmf::ticket::{Ticket, TICKET_GRACE, TICKET_RENEW};

#[test]
fn generated_stamp_validates_against_workblock() {
    let material = b"stamp-material";
    let stamp = generate_stamp(material, 0, WORKBLOCK_EXPAND_ROUNDS).expect("generated stamp");
    let workblock = stamp_workblock(material, WORKBLOCK_EXPAND_ROUNDS);
    assert!(stamp_valid(&stamp, 0, &workblock));
}

#[test]
fn cancelled_work_prevents_stamp_generation() {
    let material = b"cancel-me";
    cancel_work(material);
    let stamp = generate_stamp(material, 8, WORKBLOCK_EXPAND_ROUNDS);
    assert!(stamp.is_none());
}

#[test]
fn ticket_helpers_cover_grace_renew_and_stamp_derivation() {
    let expires = 1_000_000.0;
    let ticket = Ticket::new(expires, vec![0x33; TICKET_LENGTH]);

    assert!(ticket.is_valid_with_grace(expires + TICKET_GRACE - 1.0));
    assert!(!ticket.is_valid_with_grace(expires + TICKET_GRACE + 1.0));

    assert!(!ticket.needs_renewal(expires - TICKET_RENEW - 10.0));
    assert!(ticket.needs_renewal(expires - TICKET_RENEW + 10.0));

    let stamp = ticket.stamp_for_message(&[0x22; 32]);
    assert_eq!(stamp.len(), TICKET_LENGTH);
}
