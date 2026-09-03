//! LXMF stamps (proof-of-work), ported from Python `LXMF.LXStamper` and the
//! stamp and ticket members of `LXMF.LXMessage`.
//!
//! Issue #519 audit: a stamp is **not** a salt or nonce and plays no role in
//! the cryptographic envelope — the message payload is encrypted separately
//! via ephemeral-X25519 + Fernet before the stamp is appended in cleartext.
//! It is an anti-spam proof-of-work value that propagation nodes validate on
//! every inbound client transfer (`LXMRouter.propagation_packet` →
//! `validate_pn_stamps`) and that a delivery destination validates when it
//! enforces a cost (`LXMessage.validate_stamp`). A fixed all-zero stamp has
//! negligible work value, so nodes enforcing a stamp cost (Python default:
//! target 16, flexibility 3, i.e. minimum accepted 13) reject such messages
//! with `ERROR_INVALID_STAMP`.
//!
//! Three workblock sizes exist, one per stamp kind — delivery stamps
//! ([`WORKBLOCK_EXPAND_ROUNDS`]), propagation stamps
//! ([`PROPAGATION_WORKBLOCK_EXPAND_ROUNDS`]) and peering keys
//! ([`PEERING_WORKBLOCK_EXPAND_ROUNDS`]) — over the same [`stamp_workblock`],
//! [`stamp_valid`] and [`stamp_value`] primitives.

mod delivery;
#[cfg(test)]
mod tests;

pub use delivery::{
    generate_peering_key, generate_stamp, generate_stamp_until_cancelled, invalid_stamp_value,
    ticket_stamp, validate_peering_key, validate_stamp, COST_TICKET,
    PEERING_WORKBLOCK_EXPAND_ROUNDS, TICKET_EXPIRY_SECS, TICKET_GRACE_SECS, TICKET_INTERVAL_SECS,
    TICKET_LENGTH, TICKET_RENEW_SECS, WORKBLOCK_EXPAND_ROUNDS,
};

use alloc::vec::Vec;

use hkdf::Hkdf;
use sha2::{Digest, Sha256};

/// Size in bytes of an LXMF propagation stamp
/// (`RNS.Identity.HASHLENGTH // 8`).
pub const PROPAGATION_STAMP_SIZE: usize = 32;

/// Default stamp cost target used when the relay's announced cost is not
/// known. Mirrors Python `LXMRouter.PROPAGATION_COST` (16); a stamp at
/// this value also satisfies the default minimum accepted cost
/// (`PROPAGATION_COST - PROPAGATION_COST_FLEX` = 13).
pub const DEFAULT_PROPAGATION_STAMP_COST: u32 = 16;

/// `LXStamper.WORKBLOCK_EXPAND_ROUNDS_PN`: the propagation-stamp workblock.
pub const PROPAGATION_WORKBLOCK_EXPAND_ROUNDS: usize = 1000;

/// Maximum attainable stamp cost: the stamp value is the number of leading
/// zero bits of a SHA-256 digest, so it can never exceed 256. Requests
/// above this limit are rejected immediately instead of mining forever.
pub const MAX_STAMP_COST: u32 = 256;

/// `LXMessage.LXMF_OVERHEAD`: 2 destination hashes + signature + timestamp
/// + msgpack struct overhead.
const LXMF_OVERHEAD: usize = (2 * 16) + 64 + 8 + 8;

/// How many nonces are tried between two cancellation checks.
const CANCEL_CHECK_MASK: u64 = 0x3ff;

/// Generates a propagation stamp reaching `stamp_cost` for the given
/// transient id, mirroring `LXStamper.generate_stamp` with
/// `WORKBLOCK_EXPAND_ROUNDS_PN`. Returns `None` when `stamp_cost` exceeds
/// [`MAX_STAMP_COST`] (unattainable, would otherwise mine forever) or if
/// the nonce space is exhausted, which is unreachable for realistic costs.
pub fn generate_propagation_stamp(transient_id: &[u8; 32], stamp_cost: u32) -> Option<Vec<u8>> {
    generate_propagation_stamp_until_cancelled(transient_id, stamp_cost, || false)
}

/// [`generate_propagation_stamp`] that gives up as soon as `cancelled`
/// returns `true` — `LXStamper.cancel_work` for a search running on a
/// caller-owned thread. Polled every 1024 nonces.
pub fn generate_propagation_stamp_until_cancelled(
    transient_id: &[u8; 32],
    stamp_cost: u32,
    cancelled: impl FnMut() -> bool,
) -> Option<Vec<u8>> {
    generate_propagation_stamp_with_value_until_cancelled(transient_id, stamp_cost, cancelled)
        .map(|(stamp, _value)| stamp)
}

/// [`generate_propagation_stamp_until_cancelled`] that also returns the
/// stamp's work value, which `LXStamper.generate_stamp` reports alongside
/// the stamp.
pub fn generate_propagation_stamp_with_value_until_cancelled(
    transient_id: &[u8; 32],
    stamp_cost: u32,
    mut cancelled: impl FnMut() -> bool,
) -> Option<(Vec<u8>, u32)> {
    if stamp_cost > MAX_STAMP_COST {
        return None;
    }

    let workblock = stamp_workblock(transient_id, PROPAGATION_WORKBLOCK_EXPAND_ROUNDS);
    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut stamp = alloc::vec![0u8; PROPAGATION_STAMP_SIZE];
    let mut nonce = 0u64;

    loop {
        if nonce & CANCEL_CHECK_MASK == 0 && cancelled() {
            return None;
        }
        stamp[..8].copy_from_slice(&nonce.to_le_bytes());
        let value = stamp_value_with_prefix(&workblock_hasher, &stamp);
        if value >= stamp_cost {
            return Some((stamp, value));
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}

/// Validates the propagation stamp trailing a transient payload, mirroring
/// `LXStamper.validate_pn_stamp`: the last 32 bytes are the stamp and the
/// transient id is the full hash of the preceding data. Returns the
/// stamp's work value when it reaches `target_cost`.
pub fn validate_propagation_stamp(transient_data: &[u8], target_cost: u32) -> Option<u32> {
    if target_cost > MAX_STAMP_COST {
        return None;
    }

    if transient_data.len() <= LXMF_OVERHEAD + PROPAGATION_STAMP_SIZE {
        return None;
    }

    let (lxm_data, stamp) = transient_data.split_at(transient_data.len() - PROPAGATION_STAMP_SIZE);
    let transient_id = Sha256::digest(lxm_data);
    let workblock = stamp_workblock(transient_id.as_slice(), PROPAGATION_WORKBLOCK_EXPAND_ROUNDS);
    if stamp_valid(stamp, target_cost, &workblock) {
        Some(stamp_value(&workblock, stamp))
    } else {
        None
    }
}

/// `LXStamper.stamp_workblock`: `expand_rounds` HKDF expansions of 256
/// bytes each, keyed by the material with per-round hashed salts.
pub fn stamp_workblock(material: &[u8], expand_rounds: usize) -> Vec<u8> {
    let mut workblock = Vec::with_capacity(expand_rounds * 256);
    for n in 0..expand_rounds {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed = rmp_serde::to_vec(&n).expect("msgpack encode LXMF stamp workblock round");
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).expect("hkdf expand for LXMF stamp workblock");
        workblock.extend_from_slice(&okm);
    }
    workblock
}

/// `LXStamper.stamp_valid`: whether `stamp` reaches `target_cost` against
/// `workblock`.
pub fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    stamp_value(workblock, stamp) >= target_cost
}

/// `LXStamper.stamp_value`: leading zero bits of `sha256(workblock+stamp)`.
pub fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(workblock);
    hasher.update(stamp);
    stamp_value_from_hash(hasher.finalize().as_slice())
}

/// [`stamp_value`] against a hasher that has already absorbed the workblock,
/// so a search does not re-read the workblock per candidate.
pub(super) fn stamp_value_with_prefix(workblock_hasher: &Sha256, stamp: &[u8]) -> u32 {
    let mut hasher = workblock_hasher.clone();
    hasher.update(stamp);
    stamp_value_from_hash(hasher.finalize().as_slice())
}

fn stamp_value_from_hash(hash: &[u8]) -> u32 {
    let mut value = 0u32;
    for byte in hash {
        if *byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }
    value
}
