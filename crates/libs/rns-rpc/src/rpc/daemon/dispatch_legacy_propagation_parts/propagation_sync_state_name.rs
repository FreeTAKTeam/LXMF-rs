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

use lxmf_propagation::validation::*;

fn propagation_offer_throttle_key(peer: &str) -> Option<String> {
    let peer = peer.trim().to_ascii_lowercase();
    if peer.is_empty() {
        return None;
    }
    Some(format!("offer:{peer}"))
}
