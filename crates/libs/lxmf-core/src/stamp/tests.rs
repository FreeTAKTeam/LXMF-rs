use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use super::{
    generate_peering_key, generate_propagation_stamp, generate_propagation_stamp_until_cancelled,
    generate_propagation_stamp_with_value_until_cancelled, generate_stamp,
    generate_stamp_until_cancelled, invalid_stamp_value, stamp_value, stamp_workblock,
    ticket_stamp, validate_peering_key, validate_propagation_stamp, validate_stamp, COST_TICKET,
    DEFAULT_PROPAGATION_STAMP_COST, MAX_STAMP_COST, PEERING_WORKBLOCK_EXPAND_ROUNDS,
    PROPAGATION_STAMP_SIZE, PROPAGATION_WORKBLOCK_EXPAND_ROUNDS, TICKET_LENGTH,
    WORKBLOCK_EXPAND_ROUNDS,
};

fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

fn from_hex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex fixture"))
        .collect()
}

#[test]
fn default_propagation_stamp_cost_matches_python_lxmrouter_default() {
    // LXMRouter.PROPAGATION_COST — stamps at this value satisfy the
    // default minimum accepted cost (16 - 3 = 13).
    assert_eq!(DEFAULT_PROPAGATION_STAMP_COST, 16);
}

#[test]
fn workblock_round_constants_match_python_lxstamper() {
    assert_eq!(WORKBLOCK_EXPAND_ROUNDS, 3000);
    assert_eq!(PROPAGATION_WORKBLOCK_EXPAND_ROUNDS, 1000);
    assert_eq!(PEERING_WORKBLOCK_EXPAND_ROUNDS, 25);
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn unattainable_stamp_costs_are_rejected_before_mining() {
    // A SHA-256 digest has at most 256 leading zero bits, so costs
    // above 256 can never be reached; generation and validation must
    // fail fast instead of hanging at full CPU.
    let transient_id = sha256_array(b"unattainable");
    assert!(generate_propagation_stamp(&transient_id, MAX_STAMP_COST + 1).is_none());
    assert!(generate_propagation_stamp(&transient_id, u32::MAX).is_none());
    assert!(generate_stamp(&transient_id, MAX_STAMP_COST + 1).is_none());
    assert!(generate_peering_key(&transient_id, MAX_STAMP_COST + 1).is_none());

    let lxm_data = alloc::vec![0x42u8; 160];
    let transient_id = sha256_array(&lxm_data);
    let stamp = generate_propagation_stamp(&transient_id, 1).expect("stamp");
    let mut transient = lxm_data;
    transient.extend_from_slice(&stamp);
    assert!(validate_propagation_stamp(&transient, MAX_STAMP_COST + 1).is_none());
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn generated_propagation_stamp_validates_at_default_minimum_accepted_cost() {
    let lxm_data = alloc::vec![0x42u8; 160];
    let transient_id = sha256_array(&lxm_data);
    let stamp = generate_propagation_stamp(&transient_id, DEFAULT_PROPAGATION_STAMP_COST)
        .expect("stamp generation succeeds for realistic costs");
    assert_eq!(stamp.len(), PROPAGATION_STAMP_SIZE);
    let mut transient = lxm_data;
    transient.extend_from_slice(&stamp);

    let value = validate_propagation_stamp(&transient, 13)
        .expect("stamp at default target cost passes the default minimum accepted cost");
    assert!(value >= DEFAULT_PROPAGATION_STAMP_COST);
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn all_zero_propagation_stamp_does_not_reliably_reach_enforced_costs() {
    // The issue-519 call site appended a fixed zero stamp: its value is
    // whatever the hash happens to give, far below the Python default
    // minimum accepted cost of 13 in practice, so default-configured
    // propagation nodes reject it. (If this ever flakes, the zero
    // stamp got luckier than 1-in-2^13 — still nothing to rely on.)
    let lxm_data = alloc::vec![0x42u8; 160];
    let mut transient = lxm_data;
    transient.extend_from_slice(&[0u8; PROPAGATION_STAMP_SIZE]);

    assert!(validate_propagation_stamp(&transient, 13).is_none());
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn propagation_stamp_validator_rejects_short_or_modified_payloads() {
    let short = alloc::vec![0u8; 64 + PROPAGATION_STAMP_SIZE];
    assert!(validate_propagation_stamp(&short, 1).is_none());

    let lxm_data = alloc::vec![0x33u8; 160];
    let transient_id = sha256_array(&lxm_data);
    let stamp = generate_propagation_stamp(&transient_id, 1).expect("stamp");
    let mut transient = lxm_data;
    transient.extend_from_slice(&stamp);
    transient[0] ^= 0x01;

    assert!(validate_propagation_stamp(&transient, 1).is_none());
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn cancellable_propagation_stamp_generation_stops_before_work_loop() {
    let transient_id = [0x44u8; 32];

    assert!(generate_propagation_stamp_until_cancelled(&transient_id, 1, || true).is_none());
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn propagation_stamp_generation_reports_value() {
    let transient_id = [0x55u8; 32];
    let (stamp, value) =
        generate_propagation_stamp_with_value_until_cancelled(&transient_id, 1, || false)
            .expect("stamp");

    let workblock = stamp_workblock(&transient_id, PROPAGATION_WORKBLOCK_EXPAND_ROUNDS);
    assert_eq!(value, stamp_value(&workblock, &stamp));
    assert!(value >= 1);
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn a_generated_delivery_stamp_validates_at_its_cost() {
    let message_id = [0x22u8; 32];
    let stamp = generate_stamp(&message_id, 4).expect("stamp");

    let value = validate_stamp(Some(&stamp), &message_id, 4, &[]).expect("valid at its cost");
    assert!(value >= 4);
    assert!(validate_stamp(Some(&stamp), &message_id, value + 1, &[]).is_none());
    assert!(validate_stamp(None, &message_id, 1, &[]).is_none(), "no stamp is no proof");
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn cancellable_stamp_generation_stops_before_work_loop() {
    let message_id = [0x22u8; 32];

    assert!(generate_stamp_until_cancelled(&message_id, 1, || true).is_none());
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn invalid_stamp_value_reports_pow_value_below_policy() {
    let message_id = [0x66u8; 32];
    let stamp = alloc::vec![0u8; 8];
    let value = invalid_stamp_value(Some(&stamp), &message_id).expect("value");
    let workblock = stamp_workblock(&message_id, WORKBLOCK_EXPAND_ROUNDS);

    assert_eq!(value, stamp_value(&workblock, &stamp));
    assert!(invalid_stamp_value(None, &message_id).is_none());
}

/// Pinned against the Python reference: `LXMessage.get_stamp` with an
/// outbound ticket, run in the installed `LXMF` package for this ticket and
/// message id.
#[test]
fn ticket_stamp_matches_the_python_reference_byte_for_byte() {
    let ticket = from_hex("000102030405060708090a0b0c0d0e0f");
    let message_id: [u8; 32] =
        from_hex("1112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30")
            .try_into()
            .expect("32 bytes");

    assert_eq!(ticket_stamp(&ticket, &message_id), from_hex("5339f237639fb450588f6f3fe8350ebf"));
    assert_eq!(ticket_stamp(&ticket, &message_id).len(), TICKET_LENGTH);

    // One bit of ticket changes the whole stamp.
    let near_miss = from_hex("000102030405060708090a0b0c0d0e0e");
    assert_eq!(ticket_stamp(&near_miss, &message_id), from_hex("c538265c004588d36b9561380285995e"));
}

/// `LXMessage.validate_stamp` tries the tickets before the workblock, and
/// a ticket-paid message is worth `COST_TICKET`, whatever the target cost.
#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn a_ticket_stamp_is_honoured_before_the_workblock_and_only_for_its_ticket() {
    let ticket = alloc::vec![0x5au8; TICKET_LENGTH];
    let message_id = [0x77u8; 32];
    let stamp = ticket_stamp(&ticket, &message_id);

    assert_eq!(
        validate_stamp(Some(&stamp), &message_id, 16, core::slice::from_ref(&ticket)),
        Some(COST_TICKET)
    );
    assert_eq!(
        validate_stamp(
            Some(&stamp),
            &message_id,
            MAX_STAMP_COST,
            &[alloc::vec![0x11u8; TICKET_LENGTH], ticket]
        ),
        Some(COST_TICKET),
        "at any price, among several tickets"
    );
    assert!(validate_stamp(Some(&stamp), &message_id, 16, &[alloc::vec![0x11u8; TICKET_LENGTH]])
        .is_none());
    assert!(
        validate_stamp(Some(&stamp), &message_id, 16, &[]).is_none(),
        "without the ticket it is no proof of work"
    );
}

#[test]
fn ticket_constants_match_python_lxmessage() {
    assert_eq!(TICKET_LENGTH, 16);
    // 0x100 is the highest work value a 256-bit digest can have, which no
    // mined stamp reaches in practice.
    assert_eq!(COST_TICKET, 0x100);
    assert_eq!(COST_TICKET, MAX_STAMP_COST);
    assert_eq!(super::TICKET_EXPIRY_SECS, 21 * 24 * 60 * 60);
    assert_eq!(super::TICKET_GRACE_SECS, 5 * 24 * 60 * 60);
    assert_eq!(super::TICKET_RENEW_SECS, 14 * 24 * 60 * 60);
    assert_eq!(super::TICKET_INTERVAL_SECS, 24 * 60 * 60);
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn peering_key_validator_accepts_generated_key_and_rejects_above_value() {
    let peering_id = [0x11u8; 32];
    let key = generate_peering_key(&peering_id, 1).expect("peering key");

    let value = validate_peering_key(&peering_id, &key, 1).expect("valid peering key");
    assert!(value >= 1);

    assert!(validate_peering_key(&peering_id, &key, value + 1).is_none());
}
