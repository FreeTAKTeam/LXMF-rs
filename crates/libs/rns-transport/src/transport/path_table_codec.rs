use super::*;

pub(in crate::transport) fn decode_python_entry(
    value: &RmpValue,
) -> Result<PythonPathEntry, RnsError> {
    let RmpValue::Array(fields) = value else {
        return Err(RnsError::InvalidArgument);
    };
    if fields.len() < 8 {
        return Err(RnsError::InvalidArgument);
    }
    let interface_hash = decode_hash(&fields[6])?;
    Ok(PythonPathEntry {
        destination: decode_address_hash(&fields[0])?,
        timestamp_secs: decode_f64(&fields[1])?,
        received_from: decode_address_hash(&fields[2])?,
        hops: decode_u8(&fields[3])?,
        expires_secs: decode_f64(&fields[4])?,
        random_blobs: decode_random_blobs(&fields[5])?,
        iface: AddressHash::new_from_hash(&interface_hash),
        interface_hash,
        packet_hash: decode_hash(&fields[7])?,
    })
}

pub(in crate::transport) fn should_replace_path(
    existing: &PathEntry,
    hops: u8,
    random_blob: &RandomBlob,
    announce_emitted: u64,
    now: Instant,
    mut mode_for_iface: impl FnMut(&AddressHash) -> Option<InterfaceMode>,
) -> bool {
    if existing.random_blobs.contains(random_blob) {
        let path_emitted = newest_random_blob_timebase(&existing.random_blobs);
        return existing.state == PathState::Unresponsive
            && hops > existing.hops
            && announce_emitted == path_emitted;
    }

    let path_emitted = newest_random_blob_timebase(&existing.random_blobs);
    if hops <= existing.hops {
        return announce_emitted > path_emitted;
    }

    path_expired(existing, now, &mut mode_for_iface) || announce_emitted > path_emitted
}

fn path_expired(
    entry: &PathEntry,
    now: Instant,
    mut mode_for_iface: impl FnMut(&AddressHash) -> Option<InterfaceMode>,
) -> bool {
    let mode = mode_for_iface(&entry.iface).unwrap_or(InterfaceMode::Full);
    path_expired_for_mode(entry, now, mode)
}

pub(in crate::transport) fn path_expired_for_mode(
    entry: &PathEntry,
    now: Instant,
    mode: InterfaceMode,
) -> bool {
    now.checked_duration_since(entry.timestamp).unwrap_or_default() >= path_timeout_for_mode(mode)
}

pub(in crate::transport) fn random_blob_timebase(random_blob: &RandomBlob) -> u64 {
    let mut emitted = [0u8; 8];
    emitted[3..].copy_from_slice(&random_blob[5..]);
    u64::from_be_bytes(emitted)
}

pub(in crate::transport) fn newest_random_blob_timebase(random_blobs: &[RandomBlob]) -> u64 {
    random_blobs.iter().map(random_blob_timebase).max().unwrap_or(0)
}

pub(in crate::transport) fn bounded_random_blobs(
    mut random_blobs: Vec<RandomBlob>,
) -> Vec<RandomBlob> {
    let remove = random_blobs.len().saturating_sub(MAX_RANDOM_BLOBS);
    if remove > 0 {
        random_blobs.drain(..remove);
    }
    random_blobs
}

fn decode_address_hash(value: &RmpValue) -> Result<AddressHash, RnsError> {
    let bytes = decode_bytes(value)?;
    if bytes.len() != ADDRESS_HASH_SIZE {
        return Err(RnsError::IncorrectHash);
    }
    let mut out = [0u8; ADDRESS_HASH_SIZE];
    out.copy_from_slice(bytes);
    Ok(AddressHash::new(out))
}

fn decode_hash(value: &RmpValue) -> Result<Hash, RnsError> {
    let bytes = decode_bytes(value)?;
    if bytes.len() != HASH_SIZE {
        return Err(RnsError::IncorrectHash);
    }
    let mut out = [0u8; HASH_SIZE];
    out.copy_from_slice(bytes);
    Ok(Hash::new(out))
}

pub(in crate::transport) fn decode_random_blobs(
    value: &RmpValue,
) -> Result<Vec<RandomBlob>, RnsError> {
    let RmpValue::Array(blobs) = value else {
        return Err(RnsError::InvalidArgument);
    };

    blobs
        .iter()
        .map(|value| {
            let bytes = decode_bytes(value)?;
            if bytes.len() != RAND_HASH_LENGTH {
                return Err(RnsError::IncorrectHash);
            }
            let mut blob = RandomBlob::default();
            blob.copy_from_slice(bytes);
            Ok(blob)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(bounded_random_blobs)
}

fn decode_bytes(value: &RmpValue) -> Result<&[u8], RnsError> {
    match value {
        RmpValue::Binary(bytes) => Ok(bytes),
        RmpValue::String(text) => text.as_str().map(str::as_bytes).ok_or(RnsError::InvalidArgument),
        _ => Err(RnsError::InvalidArgument),
    }
}

fn decode_u8(value: &RmpValue) -> Result<u8, RnsError> {
    match value {
        RmpValue::Integer(value) => value.as_u64().and_then(|value| u8::try_from(value).ok()),
        _ => None,
    }
    .ok_or(RnsError::InvalidArgument)
}

fn decode_f64(value: &RmpValue) -> Result<f64, RnsError> {
    match value {
        RmpValue::F64(value) => Some(*value),
        RmpValue::F32(value) => Some(f64::from(*value)),
        RmpValue::Integer(value) => value.as_i64().map(|value| value as f64),
        _ => None,
    }
    .ok_or(RnsError::InvalidArgument)
}

pub(in crate::transport) fn path_timeout_for_mode(mode: InterfaceMode) -> Duration {
    match mode {
        InterfaceMode::AccessPoint => AP_PATH_TIME,
        InterfaceMode::Roaming => ROAMING_PATH_TIME,
        InterfaceMode::Full
        | InterfaceMode::PointToPoint
        | InterfaceMode::Boundary
        | InterfaceMode::Gateway => DESTINATION_TIMEOUT,
    }
}

include!("path_table_default.rs");
