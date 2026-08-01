//! LXMF propagation stamps (proof-of-work), ported from Python
//! `LXMF.LXStamper`.
//!
//! Issue #519 audit: the propagation stamp is **not** a salt or nonce and
//! plays no role in the cryptographic envelope — the message payload is
//! encrypted separately via ephemeral-X25519 + Fernet before the stamp is
//! appended in cleartext. It is an anti-spam proof-of-work value that
//! propagation nodes validate on every inbound client transfer
//! (`LXMRouter.propagation_packet` → `validate_pn_stamps`). A fixed
//! all-zero stamp has negligible work value, so nodes enforcing a stamp
//! cost (Python default: target 16, flexibility 3, i.e. minimum accepted
//! 13) reject such messages with `ERROR_INVALID_STAMP`.

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

/// `LXStamper.WORKBLOCK_EXPAND_ROUNDS_PN`.
const PROPAGATION_WORKBLOCK_EXPAND_ROUNDS: usize = 1000;

/// Maximum attainable stamp cost: the stamp value is the number of leading
/// zero bits of a SHA-256 digest, so it can never exceed 256. Requests
/// above this limit are rejected immediately instead of mining forever.
pub const MAX_STAMP_COST: u32 = 256;

/// `LXMessage.LXMF_OVERHEAD`: 2 destination hashes + signature + timestamp
/// + msgpack struct overhead.
const LXMF_OVERHEAD: usize = (2 * 16) + 64 + 8 + 8;

/// Generates a propagation stamp reaching `stamp_cost` for the given
/// transient id, mirroring `LXStamper.generate_stamp` with
/// `WORKBLOCK_EXPAND_ROUNDS_PN`. Returns `None` when `stamp_cost` exceeds
/// [`MAX_STAMP_COST`] (unattainable, would otherwise mine forever) or if
/// the nonce space is exhausted, which is unreachable for realistic costs.
pub fn generate_propagation_stamp(transient_id: &[u8; 32], stamp_cost: u32) -> Option<Vec<u8>> {
    if stamp_cost > MAX_STAMP_COST {
        return None;
    }

    let workblock = stamp_workblock(transient_id, PROPAGATION_WORKBLOCK_EXPAND_ROUNDS);
    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut stamp = alloc::vec![0u8; PROPAGATION_STAMP_SIZE];
    let mut nonce = 0u64;

    loop {
        stamp[..8].copy_from_slice(&nonce.to_le_bytes());
        if stamp_value_with_prefix(&workblock_hasher, &stamp) >= stamp_cost {
            return Some(stamp);
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
fn stamp_workblock(material: &[u8], expand_rounds: usize) -> Vec<u8> {
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

/// `LXStamper.stamp_valid`.
fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    stamp_value(workblock, stamp) >= target_cost
}

/// `LXStamper.stamp_value`: leading zero bits of `sha256(workblock+stamp)`.
fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(workblock);
    hasher.update(stamp);
    stamp_value_from_hash(hasher.finalize().as_slice())
}

fn stamp_value_with_prefix(workblock_hasher: &Sha256, stamp: &[u8]) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::{
        generate_propagation_stamp, validate_propagation_stamp, DEFAULT_PROPAGATION_STAMP_COST,
        PROPAGATION_STAMP_SIZE,
    };
    use sha2::{Digest, Sha256};

    fn sha256_array(data: &[u8]) -> [u8; 32] {
        let digest = Sha256::digest(data);
        let mut out = [0u8; 32];
        out.copy_from_slice(digest.as_slice());
        out
    }

    #[test]
    fn default_propagation_stamp_cost_matches_python_lxmrouter_default() {
        // LXMRouter.PROPAGATION_COST — stamps at this value satisfy the
        // default minimum accepted cost (16 - 3 = 13).
        assert_eq!(DEFAULT_PROPAGATION_STAMP_COST, 16);
    }

    #[test]
    #[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
    fn unattainable_stamp_costs_are_rejected_before_mining() {
        // A SHA-256 digest has at most 256 leading zero bits, so costs
        // above 256 can never be reached; generation and validation must
        // fail fast instead of hanging at full CPU.
        let transient_id = sha256_array(b"unattainable");
        assert!(generate_propagation_stamp(&transient_id, 257).is_none());
        assert!(generate_propagation_stamp(&transient_id, u32::MAX).is_none());

        let lxm_data = alloc::vec![0x42u8; 160];
        let transient_id = sha256_array(&lxm_data);
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("stamp");
        let mut transient = lxm_data;
        transient.extend_from_slice(&stamp);
        assert!(validate_propagation_stamp(&transient, 257).is_none());
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
}
