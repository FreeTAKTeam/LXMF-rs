use super::*;

use reticulum_daemon::lxmf_stamps::validate_peering_key;

use rns_transport::destination::DestinationName;

use sha2::Digest;

pub(super) fn handle_message_get_request(
    daemon: &RpcDaemon,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_no_access: u8,
    error_invalid_data: u8,
) -> ControlResponse {
    if !delivery_identity_allowed(daemon, remote_identity) {
        return ControlResponse::Code(error_no_access);
    }
    let Some(rmpv::Value::Array(entries)) = data else {
        return ControlResponse::Code(error_invalid_data);
    };
    if entries.len() < 2 {
        return ControlResponse::Code(error_invalid_data);
    }
    let remote_delivery_hash = delivery_destination_hash_for_identity(remote_identity);
    let remote_propagation_hash =
        hex::encode(propagation_destination_hash_for_identity(remote_identity));
    if entries.first().is_some_and(rmpv::Value::is_nil)
        && entries.get(1).is_some_and(rmpv::Value::is_nil)
    {
        let mut available = Vec::new();
        for (transient_id, size) in
            daemon.list_propagation_payloads_for_destination(&remote_delivery_hash)
        {
            let transient_id_hex = hex::encode(transient_id.as_slice());
            let completed = match daemon.has_peer_completed_propagation_mark(
                remote_propagation_hash.as_str(),
                transient_id_hex.as_str(),
            ) {
                Ok(completed) => completed,
                Err(_) => return ControlResponse::Code(error_no_access),
            };
            if !completed {
                available.push((transient_id, size));
            }
        }
        if !available.is_empty()
            && daemon.record_propagation_offer_peer(remote_propagation_hash.as_str()).is_err()
        {
            return ControlResponse::Code(error_no_access);
        }
        return ControlResponse::Rmpv(rmpv::Value::Array(
            available
                .into_iter()
                .map(|(transient_id, _size)| rmpv::Value::Binary(transient_id))
                .collect(),
        ));
    }

    let haves = match entries.get(1) {
        Some(value) if value.is_nil() => Vec::new(),
        Some(rmpv::Value::Array(values)) => binary_id_list(values),
        _ => return ControlResponse::Code(error_invalid_data),
    };
    if !haves.is_empty() {
        let have_ids = haves.iter().map(hex::encode).collect::<Vec<_>>();
        let matched_haves = daemon
            .list_propagation_payloads_for_destination(&remote_delivery_hash)
            .into_iter()
            .filter_map(|(transient_id, _size)| {
                haves
                    .iter()
                    .any(|have| have.as_slice() == transient_id.as_slice())
                    .then(|| hex::encode(transient_id))
            })
            .collect::<Vec<_>>();
        if !matched_haves.is_empty()
            && daemon.record_propagation_offer_peer(remote_propagation_hash.as_str()).is_err()
        {
            return ControlResponse::Code(error_no_access);
        }
        if !daemon.current_propagation_state().retain_synced_on_node {
            daemon.purge_propagation_payloads_for_destination(&remote_delivery_hash, &haves);
        }
        for transient_id in have_ids {
            let known_peer_work = matched_haves
                .iter()
                .any(|matched| matched.eq_ignore_ascii_case(transient_id.as_str()))
                || match daemon.has_peer_propagation_mark(
                    remote_propagation_hash.as_str(),
                    transient_id.as_str(),
                ) {
                    Ok(value) => value,
                    Err(_) => return ControlResponse::Code(error_no_access),
                };
            if !known_peer_work {
                continue;
            }
            if daemon
                .record_existing_peer_received_propagation(
                    remote_propagation_hash.as_str(),
                    transient_id.as_str(),
                )
                .is_err()
            {
                return ControlResponse::Code(error_no_access);
            }
        }
    }

    if entries.first().is_some_and(rmpv::Value::is_nil) {
        return ControlResponse::Bool(true);
    }

    let wants = match entries.first() {
        Some(rmpv::Value::Array(values)) => binary_id_list(values),
        _ => return ControlResponse::Code(error_invalid_data),
    };
    let mut retryable_wants = Vec::with_capacity(wants.len());
    for wanted in wants {
        let transient_id = hex::encode(wanted.as_slice());
        let completed = match daemon.has_peer_completed_propagation_mark(
            remote_propagation_hash.as_str(),
            transient_id.as_str(),
        ) {
            Ok(completed) => completed,
            Err(_) => return ControlResponse::Code(error_no_access),
        };
        if !completed {
            retryable_wants.push(wanted);
        }
    }
    if retryable_wants.is_empty() {
        return ControlResponse::Rmpv(rmpv::Value::Array(Vec::new()));
    }
    let transfer_limit_bytes = entries.get(2).and_then(parse_transfer_limit_bytes);
    let preview = daemon.preview_propagation_payloads_for_destination_with_ids(
        &remote_delivery_hash,
        &retryable_wants,
        transfer_limit_bytes,
    );
    let transfer_limited = daemon.transfer_limited_propagation_payload_ids_for_destination(
        &remote_delivery_hash,
        &retryable_wants,
        transfer_limit_bytes,
    );
    if (!preview.is_empty() || !transfer_limited.is_empty())
        && daemon.record_propagation_offer_peer(remote_propagation_hash.as_str()).is_err()
    {
        return ControlResponse::Code(error_no_access);
    }
    let fetched = daemon.fetch_propagation_payloads_for_destination_with_ids(
        &remote_delivery_hash,
        &retryable_wants,
        transfer_limit_bytes,
    );
    for (transient_id, _) in &fetched {
        if daemon
            .record_peer_transferred_propagation(remote_propagation_hash.as_str(), transient_id)
            .is_err()
        {
            return ControlResponse::Code(error_no_access);
        }
    }
    for transient_id in &transfer_limited {
        if daemon
            .record_peer_transfer_limited_propagation(
                remote_propagation_hash.as_str(),
                transient_id.as_str(),
            )
            .is_err()
        {
            return ControlResponse::Code(error_no_access);
        }
    }
    ControlResponse::Rmpv(rmpv::Value::Array(
        fetched.into_iter().map(|(_transient_id, payload)| rmpv::Value::Binary(payload)).collect(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_offer_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    link_id: &AddressHash,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_no_access: u8,
    error_invalid_key: u8,
    error_invalid_data: u8,
    error_throttled: u8,
) -> ControlResponse {
    let remote_propagation_hash = propagation_destination_hash_for_identity(remote_identity);
    let remote_propagation_hash_hex = hex::encode(remote_propagation_hash);
    if daemon.propagation_peer_is_throttled(remote_propagation_hash_hex.as_str()) {
        return ControlResponse::Code(error_throttled);
    }
    let propagation_state = daemon.current_propagation_state();
    if propagation_state.from_static_only
        && !propagation_state
            .static_peers
            .iter()
            .any(|peer| peer.eq_ignore_ascii_case(remote_propagation_hash_hex.as_str()))
    {
        return ControlResponse::Code(error_no_access);
    }
    let Some(rmpv::Value::Array(entries)) = data else {
        return ControlResponse::Code(error_invalid_data);
    };
    if entries.len() < 2 {
        return ControlResponse::Rmpv(rmpv::Value::Nil);
    }
    let peering_key = match entries.first() {
        Some(rmpv::Value::Binary(bytes)) => bytes.as_slice(),
        _ => return ControlResponse::Code(error_invalid_data),
    };
    let transient_ids = match entries.get(1) {
        Some(rmpv::Value::Array(values)) => values,
        _ => return ControlResponse::Code(error_invalid_data),
    };
    if daemon.propagation_peer_offer_is_throttled(remote_propagation_hash_hex.as_str()) {
        return ControlResponse::Code(error_throttled);
    }
    let peering_cost = daemon.current_propagation_state().peering_cost.unwrap_or_else(|| {
        reticulum_daemon::announce_names::PropagationNodeAnnounceConfig::default().peering_cost
    });
    let mut peering_id = Vec::with_capacity(32);
    peering_id.extend_from_slice(control.local_identity_hash.as_slice());
    peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
    if validate_peering_key(peering_id.as_slice(), peering_key, peering_cost).is_none() {
        if transient_ids.iter().all(
            |transient_id| matches!(transient_id, rmpv::Value::Binary(bytes) if bytes.len() == 32),
        ) {
            daemon.throttle_propagation_peer_offer(remote_propagation_hash_hex.as_str());
        }
        return ControlResponse::Code(error_invalid_key);
    }

    let mut offered_ids = Vec::with_capacity(transient_ids.len());
    let mut seen_offered_ids = std::collections::HashSet::with_capacity(transient_ids.len());
    for transient_id in transient_ids {
        let rmpv::Value::Binary(bytes) = transient_id else {
            return ControlResponse::Code(error_invalid_data);
        };
        if bytes.len() != 32 {
            return ControlResponse::Code(error_invalid_data);
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
            return ControlResponse::Code(error_no_access);
        }
    }
    if let Ok(mut guard) = control.validated_peer_links.lock() {
        guard.insert(*link_id);
    }

    daemon.throttle_propagation_peer_offer(remote_propagation_hash_hex.as_str());
    if wanted.len() == offered_ids.len()
        && !daemon.propagation_peer_admission_allowed(remote_propagation_hash_hex.as_str())
    {
        return ControlResponse::Rmpv(rmpv::Value::Array(Vec::new()));
    }

    if wanted.is_empty() {
        return ControlResponse::Bool(false);
    }
    if wanted.len() == offered_ids.len() {
        ControlResponse::Bool(true)
    } else {
        ControlResponse::Rmpv(rmpv::Value::Array(
            wanted.into_iter().map(rmpv::Value::Binary).collect(),
        ))
    }
}

fn binary_id_list(values: &[rmpv::Value]) -> Vec<Vec<u8>> {
    values
        .iter()
        .filter_map(|value| match value {
            rmpv::Value::Binary(bytes) if bytes.len() == 32 => Some(bytes.clone()),
            _ => None,
        })
        .collect()
}

fn parse_transfer_limit_bytes(value: &rmpv::Value) -> Option<usize> {
    let limit = match value {
        rmpv::Value::F64(value) => Some(*value),
        rmpv::Value::F32(value) => Some((*value).into()),
        rmpv::Value::Integer(value) => value.as_f64(),
        rmpv::Value::String(value) => value.as_str()?.trim().parse::<f64>().ok(),
        rmpv::Value::Binary(value) => std::str::from_utf8(value).ok()?.trim().parse::<f64>().ok(),
        rmpv::Value::Boolean(value) => Some(f64::from(*value as u8)),
        _ => None,
    }?;
    if limit.is_nan() || limit.is_infinite() && limit.is_sign_positive() {
        None
    } else {
        Some((limit.max(0.0) * 1000.0) as usize)
    }
}

fn delivery_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    named_destination_hash_for_identity(identity, "delivery")
}

fn propagation_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    named_destination_hash_for_identity(identity, "propagation")
}

fn delivery_identity_allowed(daemon: &RpcDaemon, identity: &Identity) -> bool {
    let policy = daemon
        .handle_rpc(RpcRequest { id: 0, method: "get_delivery_policy".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.get("policy").cloned())
        .unwrap_or_else(|| json!({}));
    if !policy.get("auth_required").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    let remote_hash = hex::encode(identity.address_hash.as_slice());
    policy.get("allowed_destinations").and_then(Value::as_array).is_some_and(|entries| {
        entries
            .iter()
            .filter_map(Value::as_str)
            .any(|entry| entry.eq_ignore_ascii_case(remote_hash.as_str()))
    })
}

fn named_destination_hash_for_identity(identity: &Identity, aspect: &str) -> [u8; 16] {
    let name = DestinationName::new("lxmf", aspect);
    let hash = sha2::Sha256::new()
        .chain_update(name.as_name_hash_slice())
        .chain_update(identity.address_hash.as_slice())
        .finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash[..16]);
    out
}
