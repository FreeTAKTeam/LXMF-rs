use super::*;
use lxmf_core::stamp::validate_peering_key;

#[allow(clippy::too_many_arguments)]
pub fn handle_offer(
    daemon: &impl OfferBackend,
    local_identity_hash: &[u8; 16],
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_no_access: u8,
    error_invalid_key: u8,
    error_invalid_data: u8,
    error_throttled: u8,
) -> HandlerResponse {
    let remote_propagation_hash = propagation_destination_hash_for_identity(remote_identity);
    let remote_propagation_hash_hex = hex::encode(remote_propagation_hash);
    if daemon.propagation_peer_is_throttled(remote_propagation_hash_hex.as_str()) {
        return HandlerResponse::Code(error_throttled);
    }
    if !daemon.static_peer_allowed(remote_propagation_hash_hex.as_str()) {
        return HandlerResponse::Code(error_no_access);
    }
    let Some(rmpv::Value::Array(entries)) = data else {
        return HandlerResponse::Code(error_invalid_data);
    };
    if entries.len() < 2 {
        return HandlerResponse::Rmpv(rmpv::Value::Nil);
    }
    let peering_key = match entries.first() {
        Some(rmpv::Value::Binary(bytes)) => bytes.as_slice(),
        _ => return HandlerResponse::Code(error_invalid_data),
    };
    let transient_ids = match entries.get(1) {
        Some(rmpv::Value::Array(values)) => values,
        _ => return HandlerResponse::Code(error_invalid_data),
    };
    if daemon.propagation_peer_offer_is_throttled(remote_propagation_hash_hex.as_str()) {
        return HandlerResponse::Code(error_throttled);
    }
    let peering_cost = daemon.peering_cost();
    let mut peering_id = Vec::with_capacity(32);
    peering_id.extend_from_slice(local_identity_hash.as_slice());
    peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
    if validate_peering_key(peering_id.as_slice(), peering_key, peering_cost).is_none() {
        if transient_ids.iter().all(
            |transient_id| matches!(transient_id, rmpv::Value::Binary(bytes) if bytes.len() == 32),
        ) {
            daemon.throttle_propagation_peer_offer(remote_propagation_hash_hex.as_str());
        }
        return HandlerResponse::Code(error_invalid_key);
    }

    let mut offered_ids = Vec::with_capacity(transient_ids.len());
    let mut seen_offered_ids = std::collections::HashSet::with_capacity(transient_ids.len());
    for transient_id in transient_ids {
        let rmpv::Value::Binary(bytes) = transient_id else {
            return HandlerResponse::Code(error_invalid_data);
        };
        if bytes.len() != 32 {
            return HandlerResponse::Code(error_invalid_data);
        }
        if seen_offered_ids.insert(bytes.clone()) {
            offered_ids.push(bytes.clone());
        }
    }
    let mut wanted = Vec::new();
    for bytes in &offered_ids {
        let transient_hex = hex::encode(bytes);
        if !daemon.has_propagation_payload(transient_hex.as_str()) {
            wanted.push(bytes.clone());
        } else if daemon
            .record_peer_received_propagation(
                remote_propagation_hash_hex.as_str(),
                transient_hex.as_str(),
            )
            .is_err()
        {
            return HandlerResponse::Code(error_no_access);
        }
    }
    if daemon.mark_current_link_validated().is_err() {
        return HandlerResponse::Code(error_no_access);
    }

    daemon.throttle_propagation_peer_offer(remote_propagation_hash_hex.as_str());
    if wanted.len() == offered_ids.len()
        && !daemon.propagation_peer_admission_allowed(remote_propagation_hash_hex.as_str())
    {
        return HandlerResponse::Rmpv(rmpv::Value::Array(Vec::new()));
    }

    if wanted.is_empty() {
        return HandlerResponse::Bool(false);
    }
    if wanted.len() == offered_ids.len() {
        HandlerResponse::Bool(true)
    } else {
        HandlerResponse::Rmpv(rmpv::Value::Array(
            wanted.into_iter().map(rmpv::Value::Binary).collect(),
        ))
    }
}
