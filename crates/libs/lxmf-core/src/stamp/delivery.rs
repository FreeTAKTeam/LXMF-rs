//! Delivery stamps, tickets and peering keys — the stamp and ticket members
//! of `LXMF.LXMessage` and the peering-key helpers of `LXMF.LXStamper`.

use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use super::{stamp_valid, stamp_value, stamp_value_with_prefix, stamp_workblock, MAX_STAMP_COST};

/// `LXStamper.WORKBLOCK_EXPAND_ROUNDS`: the delivery-stamp workblock.
pub const WORKBLOCK_EXPAND_ROUNDS: usize = 3000;

/// `LXStamper.WORKBLOCK_EXPAND_ROUNDS_PEERING`: the peering-key workblock.
pub const PEERING_WORKBLOCK_EXPAND_ROUNDS: usize = 25;

/// `LXMessage.TICKET_LENGTH` (`RNS.Identity.TRUNCATED_HASHLENGTH // 8`).
pub const TICKET_LENGTH: usize = 16;

/// `LXMessage.COST_TICKET`: the stamp value recorded for a message paid by
/// ticket rather than by work — the highest value a 256-bit digest can
/// have, which no mined stamp reaches in practice.
pub const COST_TICKET: u32 = 0x100;

/// `LXMessage.TICKET_EXPIRY`: how long an issued ticket is valid, in seconds.
pub const TICKET_EXPIRY_SECS: u64 = 21 * 24 * 60 * 60;

/// `LXMessage.TICKET_GRACE`: how long past its expiry a ticket is still
/// honoured by its issuer, in seconds.
pub const TICKET_GRACE_SECS: u64 = 5 * 24 * 60 * 60;

/// `LXMessage.TICKET_RENEW`: a ticket with less validity left than this is
/// replaced by a fresh one, in seconds.
pub const TICKET_RENEW_SECS: u64 = 14 * 24 * 60 * 60;

/// `LXMessage.TICKET_INTERVAL`: an issued ticket is re-sent to the same
/// peer at most this often, in seconds.
pub const TICKET_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// How many nonces are tried between two cancellation checks.
const CANCEL_CHECK_MASK: u64 = 0x3ff;

/// The stamp a ticket derives for a message: `truncated_hash(ticket +
/// message_id)` — `LXMessage.get_stamp` when an outbound ticket is held, and
/// what `LXMessage.validate_stamp` compares against each ticket it was given.
pub fn ticket_stamp(ticket: &[u8], message_id: &[u8; 32]) -> Vec<u8> {
    let mut material = Vec::with_capacity(ticket.len() + message_id.len());
    material.extend_from_slice(ticket);
    material.extend_from_slice(message_id);
    Sha256::digest(&material)[..TICKET_LENGTH].to_vec()
}

/// Generates a delivery stamp reaching `stamp_cost` for `message_id`,
/// mirroring `LXStamper.generate_stamp`. Returns `None` when `stamp_cost`
/// exceeds [`MAX_STAMP_COST`] or the nonce space is exhausted.
pub fn generate_stamp(message_id: &[u8; 32], stamp_cost: u32) -> Option<Vec<u8>> {
    generate_stamp_until_cancelled(message_id, stamp_cost, || false)
}

/// [`generate_stamp`] that gives up as soon as `cancelled` returns `true`.
pub fn generate_stamp_until_cancelled(
    message_id: &[u8; 32],
    stamp_cost: u32,
    cancelled: impl FnMut() -> bool,
) -> Option<Vec<u8>> {
    mine(message_id, WORKBLOCK_EXPAND_ROUNDS, stamp_cost, cancelled)
}

/// `LXMessage.validate_stamp`: tickets first, then the workblock. A stamp
/// derived from one of `tickets` is worth [`COST_TICKET`]; otherwise the
/// stamp's work value is returned when it reaches `target_cost`.
pub fn validate_stamp(
    stamp: Option<&[u8]>,
    message_id: &[u8; 32],
    target_cost: u32,
    tickets: &[Vec<u8>],
) -> Option<u32> {
    let stamp = stamp?;

    if tickets.iter().any(|ticket| ticket_stamp(ticket, message_id) == stamp) {
        return Some(COST_TICKET);
    }

    if target_cost > MAX_STAMP_COST {
        return None;
    }
    let workblock = stamp_workblock(message_id, WORKBLOCK_EXPAND_ROUNDS);
    if stamp_valid(stamp, target_cost, &workblock) {
        Some(stamp_value(&workblock, stamp))
    } else {
        None
    }
}

/// The work value of a stamp that failed [`validate_stamp`], for reporting
/// how far short of the target it fell.
pub fn invalid_stamp_value(stamp: Option<&[u8]>, message_id: &[u8; 32]) -> Option<u32> {
    let stamp = stamp?;
    let workblock = stamp_workblock(message_id, WORKBLOCK_EXPAND_ROUNDS);
    Some(stamp_value(&workblock, stamp))
}

/// `LXStamper.generate_stamp` with `WORKBLOCK_EXPAND_ROUNDS_PEERING`: the
/// key a propagation node presents to peer with another.
pub fn generate_peering_key(peering_id: &[u8], target_cost: u32) -> Option<Vec<u8>> {
    mine(peering_id, PEERING_WORKBLOCK_EXPAND_ROUNDS, target_cost, || false)
}

/// `LXStamper.validate_peering_key`: the key's work value when it reaches
/// `target_cost`.
pub fn validate_peering_key(
    peering_id: &[u8],
    peering_key: &[u8],
    target_cost: u32,
) -> Option<u32> {
    if target_cost > MAX_STAMP_COST {
        return None;
    }
    let workblock = stamp_workblock(peering_id, PEERING_WORKBLOCK_EXPAND_ROUNDS);
    if stamp_valid(peering_key, target_cost, &workblock) {
        Some(stamp_value(&workblock, peering_key))
    } else {
        None
    }
}

/// The search every kind shares: an 8-byte little-endian nonce, counted up
/// until `sha256(workblock + nonce)` has `stamp_cost` leading zero bits.
fn mine(
    material: &[u8],
    expand_rounds: usize,
    stamp_cost: u32,
    mut cancelled: impl FnMut() -> bool,
) -> Option<Vec<u8>> {
    if stamp_cost > MAX_STAMP_COST {
        return None;
    }

    let workblock = stamp_workblock(material, expand_rounds);
    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut nonce = 0u64;
    loop {
        if nonce & CANCEL_CHECK_MASK == 0 && cancelled() {
            return None;
        }
        let stamp = nonce.to_le_bytes();
        if stamp_value_with_prefix(&workblock_hasher, &stamp) >= stamp_cost {
            return Some(stamp.to_vec());
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}
