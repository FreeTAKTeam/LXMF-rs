use hkdf::Hkdf;
use rns_core::hash::address_hash;
use sha2::{Digest, Sha256};

pub const FIELD_TICKET: i64 = 0x0C;
pub const TICKET_LENGTH: usize = 16;
pub const COST_TICKET: u32 = 0x100;
const WORKBLOCK_EXPAND_ROUNDS: usize = 3000;

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
    let mut nonce = 0u64;
    loop {
        let stamp = nonce.to_le_bytes().to_vec();
        if stamp_valid(&stamp, stamp_cost, &workblock) {
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
    let mut material = Vec::with_capacity(workblock.len() + stamp.len());
    material.extend_from_slice(workblock);
    material.extend_from_slice(stamp);
    let hash = Sha256::digest(&material);
    let mut value = 0u32;
    for byte in hash {
        if byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }
    value
}
