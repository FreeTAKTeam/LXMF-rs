use sha2::{Digest, Sha256};

pub use lxmf_core::stamp::PROPAGATION_STAMP_SIZE;

const PROPAGATION_STAMP_WORKBLOCK_ROUNDS: usize = 1000;

// Python rejects propagation-stamped payloads that cannot contain a minimally
// structured LXMF message before validating the trailing stamp.
const MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE: usize = 112 + PROPAGATION_STAMP_SIZE;

pub fn normalize_propagation_payload_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<(String, String), std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let (transient_id, payload) =
        normalize_propagation_payload_bytes(&transient_data, target_cost)?;
    Ok((hex::encode(transient_id), hex::encode(payload)))
}

pub fn canonical_propagation_transient_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<String, std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let transient_id = canonical_propagation_transient_bytes(&transient_data, target_cost)?;
    Ok(hex::encode(transient_id))
}

pub fn decode_propagation_payload_hex(payload_hex: &str) -> Result<Vec<u8>, std::io::Error> {
    hex::decode(payload_hex.trim()).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid propagation payload hex: {err}"),
        )
    })
}

pub fn canonical_propagation_transient_bytes(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<[u8; 32], std::io::Error> {
    if target_cost == 0 {
        let transient_hash =
            Sha256::digest(propagation_payload_hash_input(transient_data, target_cost)?);
        let mut transient_id = [0u8; 32];
        transient_id.copy_from_slice(transient_hash.as_slice());
        return Ok(transient_id);
    }

    if transient_data.len() <= MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        ));
    }

    let split_at = transient_data.len() - PROPAGATION_STAMP_SIZE;
    let lxm_data = &transient_data[..split_at];
    let stamp = &transient_data[split_at..];

    let transient_hash = Sha256::digest(lxm_data);
    let workblock = propagation_stamp_workblock(transient_hash.as_slice());
    if !propagation_stamp_valid(stamp, target_cost, workblock.as_slice()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        ));
    }

    let mut transient_id = [0u8; 32];
    transient_id.copy_from_slice(transient_hash.as_slice());
    Ok(transient_id)
}

pub fn normalize_propagation_payload_bytes(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<([u8; 32], &[u8]), std::io::Error> {
    let lxm_data = propagation_payload_hash_input(transient_data, target_cost)?;

    let transient_hash = Sha256::digest(lxm_data);
    let mut transient_id = [0u8; 32];
    transient_id.copy_from_slice(transient_hash.as_slice());
    Ok((transient_id, lxm_data))
}

pub fn propagation_payload_hash_input(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<&[u8], std::io::Error> {
    if target_cost == 0 {
        return Ok(split_propagation_stamp(transient_data)
            .map(|(lxm_data, _stamp)| lxm_data)
            .unwrap_or(transient_data));
    }

    let (lxm_data, stamp) = split_propagation_stamp(transient_data).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "invalid propagation stamp")
    })?;

    let transient_hash = Sha256::digest(lxm_data);
    let workblock = propagation_stamp_workblock(transient_hash.as_slice());
    if !propagation_stamp_valid(stamp, target_cost, workblock.as_slice()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "invalid propagation stamp",
        ));
    }

    Ok(lxm_data)
}

pub fn split_propagation_stamp(transient_data: &[u8]) -> Option<(&[u8], &[u8])> {
    if transient_data.len() <= MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE {
        return None;
    }

    let split_at = transient_data.len() - PROPAGATION_STAMP_SIZE;
    Some((&transient_data[..split_at], &transient_data[split_at..]))
}

pub fn propagation_stamp_workblock(material: &[u8]) -> Vec<u8> {
    let mut workblock = Vec::with_capacity(PROPAGATION_STAMP_WORKBLOCK_ROUNDS * 256);
    for round in 0..PROPAGATION_STAMP_WORKBLOCK_ROUNDS {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed = rmp_serde::to_vec(&round).expect("msgpack encode propagation stamp round");
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = hkdf::Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).expect("hkdf expand propagation stamp workblock");
        workblock.extend_from_slice(&okm);
    }
    workblock
}

pub fn propagation_stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    propagation_stamp_value(workblock, stamp) >= target_cost
}

pub fn propagation_stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
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
