use hkdf::Hkdf;
use rns_core::hash::address_hash;
use sha2::{Digest, Sha256};

pub const FIELD_TICKET: i64 = 0x0C;
pub const TICKET_LENGTH: usize = 16;
pub const COST_TICKET: u32 = 0x100;
const WORKBLOCK_EXPAND_ROUNDS: usize = 3000;
pub const PROPAGATION_STAMP_SIZE: usize = 32;
const PROPAGATION_WORKBLOCK_EXPAND_ROUNDS: usize = 1000;
const PEERING_WORKBLOCK_EXPAND_ROUNDS: usize = 25;
const LXMF_OVERHEAD: usize = (2 * 16) + 64 + 8 + 8;

pub fn decode_ticket_hex(ticket_hex: &str) -> Result<Vec<u8>, String> {
    let bytes = hex::decode(ticket_hex.trim())
        .map_err(|error| format!("invalid outbound ticket hex: {error}"))?;
    if bytes.len() != TICKET_LENGTH {
        return Err(format!(
            "invalid outbound ticket length {}; expected {} bytes",
            bytes.len(),
            TICKET_LENGTH
        ));
    }
    Ok(bytes)
}

pub fn ticket_stamp(ticket: &[u8], message_id: &[u8; 32]) -> Vec<u8> {
    let mut material = Vec::with_capacity(ticket.len() + message_id.len());
    material.extend_from_slice(ticket);
    material.extend_from_slice(message_id);
    address_hash(&material).to_vec()
}

pub fn generate_stamp(message_id: &[u8; 32], stamp_cost: u32) -> Option<Vec<u8>> {
    let workblock = stamp_workblock(message_id, WORKBLOCK_EXPAND_ROUNDS);
    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut nonce = 0u64;
    loop {
        let stamp = nonce.to_le_bytes().to_vec();
        if stamp_value_with_prefix(&workblock_hasher, &stamp) >= stamp_cost {
            return Some(stamp);
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}

pub fn generate_propagation_stamp(transient_id: &[u8; 32], stamp_cost: u32) -> Option<Vec<u8>> {
    let workblock = stamp_workblock(transient_id, PROPAGATION_WORKBLOCK_EXPAND_ROUNDS);
    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut stamp = vec![0u8; PROPAGATION_STAMP_SIZE];
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

pub fn generate_peering_key(peering_id: &[u8], target_cost: u32) -> Option<Vec<u8>> {
    generate_stamp_with_rounds(peering_id, target_cost, PEERING_WORKBLOCK_EXPAND_ROUNDS)
}

pub fn validate_peering_key(
    peering_id: &[u8],
    peering_key: &[u8],
    target_cost: u32,
) -> Option<u32> {
    let workblock = stamp_workblock(peering_id, PEERING_WORKBLOCK_EXPAND_ROUNDS);
    if stamp_valid(peering_key, target_cost, &workblock) {
        Some(stamp_value(&workblock, peering_key))
    } else {
        None
    }
}

pub fn validate_propagation_stamp(transient_data: &[u8], target_cost: u32) -> Option<u32> {
    if transient_data.len() <= LXMF_OVERHEAD + PROPAGATION_STAMP_SIZE {
        return None;
    }

    let lxm_data_len = transient_data.len() - PROPAGATION_STAMP_SIZE;
    let (lxm_data, stamp) = transient_data.split_at(lxm_data_len);
    let transient_id = Sha256::digest(lxm_data);
    let workblock = stamp_workblock(transient_id.as_slice(), PROPAGATION_WORKBLOCK_EXPAND_ROUNDS);
    if stamp_valid(stamp, target_cost, &workblock) {
        Some(stamp_value(&workblock, stamp))
    } else {
        None
    }
}

fn generate_stamp_with_rounds(
    material: &[u8],
    stamp_cost: u32,
    expand_rounds: usize,
) -> Option<Vec<u8>> {
    let workblock = stamp_workblock(material, expand_rounds);
    let mut workblock_hasher = Sha256::new();
    workblock_hasher.update(&workblock);
    let mut nonce = 0u64;
    loop {
        let stamp = nonce.to_le_bytes().to_vec();
        if stamp_value_with_prefix(&workblock_hasher, &stamp) >= stamp_cost {
            return Some(stamp);
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}

pub fn validate_stamp(
    stamp: Option<&[u8]>,
    message_id: &[u8; 32],
    target_cost: u32,
    tickets: &[Vec<u8>],
) -> Option<u32> {
    let stamp = stamp?;

    for ticket in tickets {
        if target_cost <= COST_TICKET && ticket_stamp(ticket.as_slice(), message_id) == stamp {
            return Some(COST_TICKET);
        }
    }

    let workblock = stamp_workblock(message_id, WORKBLOCK_EXPAND_ROUNDS);
    if stamp_valid(stamp, target_cost, &workblock) {
        Some(stamp_value(&workblock, stamp))
    } else {
        None
    }
}

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

pub fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    stamp_value(workblock, stamp) >= target_cost
}

pub fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
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
        generate_peering_key, generate_propagation_stamp, validate_peering_key,
        validate_propagation_stamp, PROPAGATION_STAMP_SIZE,
    };
    use sha2::{Digest, Sha256};

    #[test]
    fn propagation_stamp_validator_accepts_generated_transient_stamp() {
        let lxm_data = vec![0x42u8; 160];
        let transient_id = sha256_array(&lxm_data);
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("stamp");
        let mut transient = lxm_data;
        transient.extend_from_slice(&stamp);

        let value = validate_propagation_stamp(&transient, 1).expect("valid propagation stamp");
        assert!(value >= 1);
    }

    #[test]
    fn propagation_stamp_validator_rejects_short_or_modified_transient_stamp() {
        let short = vec![0u8; 64 + PROPAGATION_STAMP_SIZE];
        assert!(validate_propagation_stamp(&short, 1).is_none());

        let lxm_data = vec![0x33u8; 160];
        let transient_id = sha256_array(&lxm_data);
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("stamp");
        let mut transient = lxm_data;
        transient.extend_from_slice(&stamp);
        transient[0] ^= 0x01;

        assert!(validate_propagation_stamp(&transient, 1).is_none());
    }

    #[test]
    fn peering_key_validator_accepts_generated_key_and_rejects_above_value() {
        let peering_id = [0x11u8; 32];
        let key = generate_peering_key(&peering_id, 1).expect("peering key");

        let value = validate_peering_key(&peering_id, &key, 1).expect("valid peering key");
        assert!(value >= 1);

        assert!(validate_peering_key(&peering_id, &key, value + 1).is_none());
    }

    fn sha256_array(data: &[u8]) -> [u8; 32] {
        let digest = Sha256::digest(data);
        let mut out = [0u8; 32];
        out.copy_from_slice(digest.as_slice());
        out
    }
}
