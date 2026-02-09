use hkdf::Hkdf;
use sha2::Sha256;
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

pub type PnStampValidation = (Vec<u8>, Vec<u8>, u32, Vec<u8>);

fn cancelled_work() -> &'static Mutex<BTreeSet<Vec<u8>>> {
    static CANCELLED: OnceLock<Mutex<BTreeSet<Vec<u8>>>> = OnceLock::new();
    CANCELLED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

pub fn stamp_workblock(material: &[u8], expand_rounds: usize) -> Vec<u8> {
    let mut workblock = Vec::with_capacity(expand_rounds * 256);

    for n in 0..expand_rounds {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed = rmp_serde::to_vec(&n).unwrap();
        salt_data.extend_from_slice(&packed);
        let salt_hash = reticulum::hash::Hash::new_from_slice(&salt_data);

        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).unwrap();
        workblock.extend_from_slice(&okm);
    }

    workblock
}

pub fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let hash = reticulum::hash::Hash::new_from_slice(&[workblock, stamp].concat());
    let mut value = 0u32;

    for byte in hash.as_slice() {
        if *byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }

    value
}

pub fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    stamp_value(workblock, stamp) >= target_cost
}

pub fn validate_pn_stamp(transient_data: &[u8], target_cost: u32) -> Option<PnStampValidation> {
    let stamp_size = reticulum::hash::HASH_SIZE;
    if transient_data.len() <= stamp_size {
        return None;
    }

    let (lxm_data, stamp) = transient_data.split_at(transient_data.len() - stamp_size);
    let transient_id = reticulum::hash::Hash::new_from_slice(lxm_data)
        .to_bytes()
        .to_vec();
    let workblock = stamp_workblock(&transient_id, crate::constants::WORKBLOCK_EXPAND_ROUNDS_PN);

    if !stamp_valid(stamp, target_cost, &workblock) {
        return None;
    }

    let value = stamp_value(&workblock, stamp);
    Some((transient_id, lxm_data.to_vec(), value, stamp.to_vec()))
}

pub fn validate_peering_key(peering_id: &[u8], peering_key: &[u8], target_cost: u32) -> bool {
    let workblock = stamp_workblock(peering_id, crate::constants::WORKBLOCK_EXPAND_ROUNDS_PN);
    stamp_valid(peering_key, target_cost, &workblock)
}

pub fn cancel_work(material: &[u8]) {
    if let Ok(mut cancelled) = cancelled_work().lock() {
        cancelled.insert(material.to_vec());
    }
}

pub fn generate_stamp(material: &[u8], stamp_cost: u32, expand_rounds: usize) -> Option<Vec<u8>> {
    if let Ok(cancelled) = cancelled_work().lock() {
        if cancelled.contains(material) {
            return None;
        }
    }

    let workblock = stamp_workblock(material, expand_rounds);
    let mut nonce = 0u64;

    loop {
        if let Ok(cancelled) = cancelled_work().lock() {
            if cancelled.contains(material) {
                return None;
            }
        }

        let stamp = nonce.to_le_bytes().to_vec();
        if stamp_valid(&stamp, stamp_cost, &workblock) {
            if let Ok(mut cancelled) = cancelled_work().lock() {
                cancelled.remove(material);
            }
            return Some(stamp);
        }

        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}
