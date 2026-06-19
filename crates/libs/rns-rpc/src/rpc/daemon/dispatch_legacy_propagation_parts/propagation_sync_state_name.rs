fn propagation_sync_state_name(state: u32) -> &'static str {
    match state {
        PR_IDLE => "idle",
        PR_REQUEST_SENT => "syncing",
        PR_COMPLETE => "completed",
        PR_FAILED => "failed",
        0x01 => "path_requested",
        0x02 => "link_establishing",
        0x03 => "link_established",
        0x05 => "receiving",
        0x06 => "response_received",
        0xf0 => "no_path",
        0xf1 => "link_failed",
        0xf2 => "transfer_failed",
        0xf3 => "no_identity",
        PR_NO_ACCESS => "no_access",
        _ => "unknown",
    }
}

fn effective_transfer_limit_kb(
    peer_transfer_limit_kb: Option<f64>,
    request_transfer_limit_kb: Option<f64>,
) -> Option<f64> {
    match (peer_transfer_limit_kb, request_transfer_limit_kb) {
        (Some(peer_limit), Some(request_limit)) => Some(peer_limit.min(request_limit)),
        (Some(peer_limit), None) => Some(peer_limit),
        (None, Some(request_limit)) => Some(request_limit),
        (None, None) => None,
    }
}

fn remote_propagation_message_payload(
    message: &JsonValue,
) -> Result<Option<(Vec<u8>, String)>, std::io::Error> {
    if let Some(payload_hex) = message.get("payload_hex").and_then(JsonValue::as_str) {
        let payload = hex::decode(payload_hex.trim()).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid remote propagation payload hex: {err}"),
            )
        })?;
        return Ok(Some((payload, payload_hex.trim().to_ascii_lowercase())));
    }

    for field in ["payload", "payload_bytes"] {
        if let Some(value) = message.get(field) {
            if let Some(payload) = remote_propagation_byte_array(value, field)? {
                let payload_hex = hex::encode(payload.as_slice());
                return Ok(Some((payload, payload_hex)));
            }
        }
    }

    Ok(None)
}

fn remote_propagation_byte_array(
    value: &JsonValue,
    field: &str,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    let Some(items) = value.as_array() else {
        return if field == "payload_bytes" && value.as_u64().is_some() {
            Ok(None)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid remote propagation {field} byte array"),
            ))
        };
    };
    items
        .iter()
        .map(|item| item.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect::<Option<Vec<_>>>()
        .map(Some)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid remote propagation {field} byte array"),
            )
        })
}

fn is_remote_access_denied_error(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied
        && err.to_string() == "propagation node denied access"
}

fn remote_propagation_failure_state(err: &std::io::Error) -> u32 {
    if is_remote_access_denied_error(err) {
        PR_NO_ACCESS
    } else {
        PR_FAILED
    }
}

fn is_retryable_remote_peer_sync_error(err: &std::io::Error) -> bool {
    matches!(
        (err.kind(), err.to_string().as_str()),
        (std::io::ErrorKind::PermissionDenied, "propagation node requires identity")
            | (std::io::ErrorKind::PermissionDenied, "propagation peer invalid peering key")
            | (std::io::ErrorKind::PermissionDenied, "propagation peer invalid stamp")
            | (std::io::ErrorKind::InvalidInput, "propagation node rejected the request")
            | (std::io::ErrorKind::InvalidData, "unexpected propagation control response")
            | (std::io::ErrorKind::NotFound, "propagation peer not found")
            | (std::io::ErrorKind::TimedOut, "propagation peer timed out")
            | (std::io::ErrorKind::BrokenPipe, "propagation link closed")
            | (std::io::ErrorKind::ConnectionAborted, "propagation link closed")
            | (std::io::ErrorKind::ConnectionReset, "propagation link closed")
            | (std::io::ErrorKind::NotConnected, "propagation link closed")
            | (std::io::ErrorKind::UnexpectedEof, "propagation link closed")
    )
}

fn remote_peer_sync_failure_kind(error: &str, postpone_reason: Option<&str>) -> &'static str {
    if postpone_reason == Some("throttled") {
        return "throttled";
    }
    if error.starts_with("invalid remote propagation payload hex") {
        return "invalid_data";
    }
    match error {
        "propagation node requires identity" => "no_identity",
        "propagation peer invalid peering key" => "invalid_key",
        "propagation peer invalid stamp" => "invalid_stamp",
        "propagation node rejected the request" | "unexpected propagation control response" => {
            "invalid_data"
        }
        "propagation peer not found" => "not_found",
        "propagation peer timed out" | "propagation link closed" | "remote sync failed" => {
            "timeout"
        }
        "propagation node denied access" => "no_access",
        _ => "failed",
    }
}

fn normalize_propagation_transient_key(transient_id: &str) -> String {
    transient_id.trim().to_ascii_lowercase()
}

const PROPAGATION_STAMP_SIZE: usize = 32;

const PROPAGATION_STAMP_WORKBLOCK_ROUNDS: usize = 1000;

// Python rejects propagation-stamped payloads that cannot contain a minimally
// structured LXMF message before validating the trailing stamp.
const MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE: usize = 112 + PROPAGATION_STAMP_SIZE;

pub(super) fn normalize_propagation_payload_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<(String, String), std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let (transient_id, payload) =
        normalize_propagation_payload_bytes(&transient_data, target_cost)?;
    Ok((hex::encode(transient_id), hex::encode(payload)))
}

pub(super) fn canonical_propagation_transient_hex(
    payload_hex: &str,
    target_cost: u32,
) -> Result<String, std::io::Error> {
    let transient_data = decode_propagation_payload_hex(payload_hex)?;
    let transient_id = canonical_propagation_transient_bytes(&transient_data, target_cost)?;
    Ok(hex::encode(transient_id))
}

pub(super) fn decode_propagation_payload_hex(payload_hex: &str) -> Result<Vec<u8>, std::io::Error> {
    hex::decode(payload_hex.trim()).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid propagation payload hex: {err}"),
        )
    })
}

pub(super) fn canonical_propagation_transient_bytes(
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

pub(super) fn normalize_propagation_payload_bytes(
    transient_data: &[u8],
    target_cost: u32,
) -> Result<([u8; 32], &[u8]), std::io::Error> {
    let lxm_data = propagation_payload_hash_input(transient_data, target_cost)?;

    let transient_hash = Sha256::digest(lxm_data);
    let mut transient_id = [0u8; 32];
    transient_id.copy_from_slice(transient_hash.as_slice());
    Ok((transient_id, lxm_data))
}

pub(super) fn propagation_payload_hash_input(
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

fn propagation_offer_throttle_key(peer: &str) -> Option<String> {
    let peer = peer.trim().to_ascii_lowercase();
    if peer.is_empty() {
        return None;
    }
    Some(format!("offer:{peer}"))
}

pub(super) fn split_propagation_stamp(transient_data: &[u8]) -> Option<(&[u8], &[u8])> {
    if transient_data.len() <= MIN_PROPAGATION_STAMPED_PAYLOAD_SIZE {
        return None;
    }

    let split_at = transient_data.len() - PROPAGATION_STAMP_SIZE;
    Some((&transient_data[..split_at], &transient_data[split_at..]))
}

fn propagation_payload_matches_destination(payload: &[u8], destination: &[u8; 16]) -> bool {
    payload.len() >= 16 && &payload[..16] == destination
}

pub(super) fn propagation_stamp_workblock(material: &[u8]) -> Vec<u8> {
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

pub(super) fn propagation_stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    propagation_stamp_value(workblock, stamp) >= target_cost
}

pub(super) fn propagation_stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
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
