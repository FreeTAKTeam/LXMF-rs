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
        let available = daemon.list_propagation_payloads_for_destination(&remote_delivery_hash);
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
        daemon.purge_propagation_payloads_for_destination(&remote_delivery_hash, &haves);
        for transient_id in matched_haves {
            if daemon
                .record_peer_received_propagation(
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
    let peering_cost = daemon.current_propagation_state().peering_cost.unwrap_or_else(|| {
        reticulum_daemon::announce_names::PropagationNodeAnnounceConfig::default().peering_cost
    });
    let mut peering_id = Vec::with_capacity(32);
    peering_id.extend_from_slice(control.local_identity_hash.as_slice());
    peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
    if validate_peering_key(peering_id.as_slice(), peering_key, peering_cost).is_none() {
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
    if daemon.propagation_peer_offer_is_throttled(remote_propagation_hash_hex.as_str()) {
        return ControlResponse::Code(error_throttled);
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

#[cfg(test)]
mod tests {
    use super::*;
    use reticulum_daemon::lxmf_stamps::generate_peering_key;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn test_validated_peer_links() -> Arc<Mutex<HashSet<AddressHash>>> {
        Arc::new(Mutex::new(HashSet::new()))
    }

    fn test_link_id() -> AddressHash {
        AddressHash::new([0xA6; 16])
    }

    #[test]
    fn offer_request_returns_only_missing_transient_ids_after_peering_key_validation() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let existing = [0xAA; 32];
        let missing = [0xBB; 32];
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                b"stored propagation payload",
                hex::encode(existing).as_str(),
                &[],
            )
            .expect("store existing payload");

        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let link_id = test_link_id();
        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(existing.to_vec()),
                    rmpv::Value::Binary(missing.to_vec()),
                ]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(wanted)) = response else {
            panic!("expected partial wanted-id list");
        };
        assert_eq!(wanted, vec![rmpv::Value::Binary(missing.to_vec())]);
        assert!(control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "wanted offers should validate the link without admitting or queueing the peer"
        );
    }

    #[test]
    fn offer_request_empty_offer_does_not_queue_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let existing_payload = b"queued propagation payload";
        let existing_transient_id = hex::encode(sha2::Sha256::digest(existing_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                existing_payload,
                existing_transient_id.as_str(),
                &[],
            )
            .expect("store existing payload");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let link_id = test_link_id();
        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(Vec::new()),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
        assert!(control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())));
    }

    #[test]
    fn offer_request_all_known_offer_does_not_queue_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let known_payload = b"known propagation offer payload";
        let known_transient_id = hex::encode(sha2::Sha256::digest(known_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                known_payload,
                known_transient_id.as_str(),
                &[],
            )
            .expect("store known payload");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let link_id = test_link_id();
        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(
                    hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                )]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
        assert!(control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    known_transient_id.as_str(),
                )
                .expect("known offer mark"),
            "known offered payloads should be marked as already received from the offering peer"
        );
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("admit peer after offer");
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list admitted peer")
            .result
            .expect("list admitted peer result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("admitted peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([known_transient_id]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn offer_request_rejects_throttled_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon.throttle_propagation_peer_for_invalid_stamp(remote_propagation_hash.as_str());
        let offered = [0xBB; 32];
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF6)));
    }

    #[test]
    fn offer_request_repeated_valid_offer_is_throttled_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let offered = [0xBB; 32];
        let other_offered = [0xBC; 32];
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let first = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );
        let second = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );
        let different_offer = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(other_offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(first, ControlResponse::Bool(true)));
        assert!(matches!(second, ControlResponse::Code(0xF6)));
        assert!(matches!(different_offer, ControlResponse::Code(0xF6)));
    }

    #[test]
    fn offer_request_short_array_returns_nil_without_recording_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let link_id = test_link_id();
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Binary(vec![0xAA; 32])])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Rmpv(rmpv::Value::Nil)));
        assert!(!control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "short offer request must not create a peer record"
        );
    }

    #[test]
    fn offer_request_rejects_invalid_transient_id_without_recording_peer() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(vec![0xAA; 31])]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF4)));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "invalid offer data must not create a peer record"
        );
    }

    #[test]
    fn offer_request_rejects_mixed_invalid_offer_without_partial_queue_marks() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let known_payload = b"known before invalid offer id";
        let known_transient_id = hex::encode(sha2::Sha256::digest(known_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                known_payload,
                known_transient_id.as_str(),
                &[],
            )
            .expect("store known payload");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let link_id = test_link_id();
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(
                        hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                    ),
                    rmpv::Value::Binary(vec![0xAA; 31]),
                ]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF4)));
        assert!(!control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    known_transient_id.as_str(),
                )
                .expect("known offer mark"),
            "invalid offer data must not leave partial source-accounting queue marks"
        );
    }

    #[test]
    fn offer_request_deduplicates_missing_wanted_ids_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let known_payload = b"known before duplicate missing offers";
        let known_transient_id = hex::encode(sha2::Sha256::digest(known_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                known_payload,
                known_transient_id.as_str(),
                &[],
            )
            .expect("store known payload");
        let missing_transient = [0x64; 32];
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(
                        hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                    ),
                    rmpv::Value::Binary(missing_transient.to_vec()),
                    rmpv::Value::Binary(missing_transient.to_vec()),
                ]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(wanted)) = response else {
            panic!("expected partial wanted-id list");
        };
        assert_eq!(
            wanted,
            vec![rmpv::Value::Binary(missing_transient.to_vec())],
            "duplicate offered missing IDs should be requested once"
        );
    }

    #[test]
    fn offer_request_defers_capacity_limited_peer_admission_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                    "max_peers": 1,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-capacity-existing" })),
            })
            .expect("fill peer capacity");

        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let offered = [0xBB; 32];
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };
        let link_id = test_link_id();

        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(
            matches!(response, ControlResponse::Rmpv(rmpv::Value::Array(values)) if values.is_empty())
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "wanted offer response should not consume peer capacity before transfer admission"
        );
        assert!(
            control.validated_peer_links.lock().expect("validated peer links").contains(&link_id),
            "valid wanted offer should still validate the peering link"
        );
    }

    #[test]
    fn offer_request_capacity_limited_valid_offer_starts_throttle_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                    "max_peers": 1,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-capacity-existing" })),
            })
            .expect("fill peer capacity");

        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let offered = [0xBC; 32];
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let offer_data = || {
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ]))
        };
        let first = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            offer_data(),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );
        let second = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            offer_data(),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(
            matches!(first, ControlResponse::Rmpv(rmpv::Value::Array(values)) if values.is_empty())
        );
        assert!(matches!(second, ControlResponse::Code(0xF6)));
    }

    #[test]
    fn offer_request_rejects_non_static_peer_when_static_only() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(Vec::new()),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn offer_request_allows_static_peer_destination_hash_when_static_only() {
        let daemon = RpcDaemon::test_instance();
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": [remote_propagation_hash],
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(Vec::new()),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
    }

    #[test]
    fn offer_request_static_only_rejects_before_data_validation() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Nil),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn message_get_rejects_identity_when_delivery_auth_required() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "auth_required": true,
                    "allowed_destinations": ["not-this-identity"],
                })),
            })
            .expect("set delivery policy");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn message_get_allows_python_identity_hash_when_delivery_auth_required() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        daemon
            .handle_rpc(RpcRequest {
                id: 13,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "auth_required": true,
                    "allowed_destinations": [hex::encode(remote_identity.address_hash.as_slice())],
                })),
            })
            .expect("set delivery policy");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(
            matches!(response, ControlResponse::Rmpv(rmpv::Value::Array(values)) if values.is_empty())
        );
    }

    #[test]
    fn message_get_rejects_delivery_destination_hash_when_auth_requires_python_identity_hash() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        daemon
            .handle_rpc(RpcRequest {
                id: 14,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "auth_required": true,
                    "allowed_destinations": [
                        hex::encode(delivery_destination_hash_for_identity(&remote_identity))
                    ],
                })),
            })
            .expect("set delivery policy");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn message_get_lists_fetches_and_purges_remote_delivery_payloads() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let other_delivery_hash = [0x44; 16];
        let wanted = [0x22; 32];
        let have = [0x33; 32];
        let ignored = [0x55; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation lxm");
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation lxm");
        let mut ignored_payload = other_delivery_hash.to_vec();
        ignored_payload.extend_from_slice(b" other recipient");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                hex::encode(have).as_str(),
                &[],
            )
            .expect("store have payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                ignored_payload.as_slice(),
                hex::encode(ignored).as_str(),
                &[],
            )
            .expect("store ignored payload");

        let list_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(available)) = list_response else {
            panic!("expected available transient id list");
        };
        assert_eq!(
            available,
            vec![rmpv::Value::Binary(wanted.to_vec()), rmpv::Value::Binary(have.to_vec())]
        );

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
        assert!(!daemon.has_propagation_payload(hex::encode(have).as_str()));
        assert!(daemon.has_propagation_payload(hex::encode(ignored).as_str()));
    }

    #[test]
    fn message_get_haves_mark_requesting_peer_received_across_purge_and_reingest() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let have = [0x36; 32];
        let have_hex = hex::encode(have);
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation accounting lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("store have payload");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Bool(true)));
        assert!(!daemon.has_propagation_payload(have_hex.as_str()));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("completed propagation mark lookup"),
            "message-get haves should be remembered as peer-received after local purge"
        );

        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("reingest have payload");
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("completed propagation mark after reingest"),
            "reingesting a purged payload must not forget that the peer already has it"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 15, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(
            row["messages"]["unhandled_ids"],
            json!([]),
            "reingested haves should not be queued back to the declaring peer"
        );
    }

    #[test]
    fn message_get_haves_preserve_other_peer_completed_marks_across_purge_and_reingest() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let other_peer = "ab".repeat(16);
        let have = [0x37; 32];
        let have_hex = hex::encode(have);
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" haves should preserve other peer accounting");
        daemon.record_propagation_offer_peer(other_peer.as_str()).expect("activate other peer");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("store have payload");
        daemon
            .record_peer_transferred_propagation(other_peer.as_str(), have_hex.as_str())
            .expect("mark other peer transferred");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Bool(true)));
        assert!(!daemon.has_propagation_payload(have_hex.as_str()));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(other_peer.as_str(), have_hex.as_str())
                .expect("other peer completed mark"),
            "purging one peer's haves must not erase another peer's completed mark"
        );
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("requesting peer completed mark"),
            "requesting peer should still be marked completed after purge"
        );

        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("reingest have payload");
        let peers = daemon
            .handle_rpc(RpcRequest { id: 16, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(other_peer.as_str()))
            .expect("other peer row");
        assert_eq!(
            row["messages"]["unhandled_ids"],
            json!([]),
            "reingested payload must not be requeued to a peer that already completed it"
        );
    }

    #[test]
    fn message_get_marks_served_wanted_payloads_transferred_for_peer() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x24; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation accounting lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
        assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_admits_served_peer_for_transfer_accounting_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x25; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation accounting without prior offer");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("served peer row");
        assert_eq!(row["peer_type"].as_str(), Some("manual"));
        assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
        assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_rejected_peer_does_not_count_or_mark_served_payload() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable static-only propagation");
        let wanted = [0x26; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" rejected peer should not be counted as served");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(fetch_response, ControlResponse::Code(0xF1)));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "rejected message-get peer must not create a peer record"
        );
        let status = daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(
            status["propagation"]["client_propagation_messages_served"].as_u64(),
            Some(0),
            "rejected message-get peer must not increment served counters"
        );
    }

    #[test]
    fn message_get_rejected_peer_cannot_list_fetchable_payload_ids() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable static-only propagation");
        let wanted = [0x27; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" rejected peer should not list payload ids");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let list_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(matches!(list_response, ControlResponse::Code(0xF1)));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "rejected message-get list must not create a peer record"
        );
    }

    #[test]
    fn message_get_rejected_peer_cannot_purge_haves() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable static-only propagation");
        let have = [0x28; 32];
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" rejected peer should not purge haves");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                hex::encode(have).as_str(),
                &[],
            )
            .expect("store have payload");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
        assert!(
            daemon.has_propagation_payload(hex::encode(have).as_str()),
            "rejected message-get haves must not purge queued payload"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "rejected message-get haves must not create a peer record"
        );
    }

    #[test]
    fn message_get_ignores_malformed_transient_ids_inside_lists_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x5A; 32];
        let have = [0x5B; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation lxm");
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                hex::encode(have).as_str(),
                &[],
            )
            .expect("store have payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(vec![0x01; 31]),
                    rmpv::Value::Integer(7.into()),
                    rmpv::Value::Binary(wanted.to_vec()),
                ]),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(vec![0x02; 33]),
                    rmpv::Value::String("not-a-transient-id".into()),
                    rmpv::Value::Binary(have.to_vec()),
                ]),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
        assert!(!daemon.has_propagation_payload(hex::encode(have).as_str()));
    }

    #[test]
    fn message_get_purges_haves_before_rejecting_invalid_wants_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let have = [0x5C; 32];
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                hex::encode(have).as_str(),
                &[],
            )
            .expect("store have payload");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Integer(7.into()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF4)));
        assert!(
            !daemon.has_propagation_payload(hex::encode(have).as_str()),
            "Python purges haves before later malformed wants abort the request"
        );
    }

    #[test]
    fn message_get_zero_transfer_limit_skips_payload_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x66; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(0u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "zero transfer limit should behave as a real zero-byte budget"
        );
    }

    #[test]
    fn message_get_transfer_limited_wanted_payload_marks_peer_completed_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x67; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(&[0x42; 2_000]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(1u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(messages.is_empty());
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    hex::encode(wanted).as_str(),
                )
                .expect("completed propagation mark lookup"),
            "transfer-limited message-get wants should be completed for this peer"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_transfer_limited_retry_does_not_serve_completed_payload() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x6A; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(&[0x42; 2_000]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let limited_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(1u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(limited_messages)) = limited_response else {
            panic!("expected limited fetched message list");
        };
        assert!(limited_messages.is_empty());

        let retry_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(retry_messages)) = retry_response else {
            panic!("expected retry fetched message list");
        };
        assert!(
            retry_messages.is_empty(),
            "transfer-limited completed wants should not be served on a later retry"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_cumulative_budget_skip_keeps_later_payload_retryable_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let first = [0x68; 32];
        let second = [0x69; 32];
        let mut first_payload = remote_delivery_hash.to_vec();
        first_payload.extend_from_slice(&[0x42; 900]);
        let mut second_payload = remote_delivery_hash.to_vec();
        second_payload.extend_from_slice(&[0x43; 900]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                first_payload.as_slice(),
                hex::encode(first).as_str(),
                &[],
            )
            .expect("store first payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                second_payload.as_slice(),
                hex::encode(second).as_str(),
                &[],
            )
            .expect("store second payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(first.to_vec()),
                    rmpv::Value::Binary(second.to_vec()),
                ]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(1u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages.len(), 1);
        assert!(daemon
            .has_peer_completed_propagation_mark(
                remote_propagation_hash.as_str(),
                hex::encode(first).as_str(),
            )
            .expect("first completed propagation mark lookup"));
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    hex::encode(second).as_str(),
                )
                .expect("second completed propagation mark lookup"),
            "payloads skipped only by the cumulative response budget should remain retryable"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 13, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(first)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([hex::encode(second)]));
    }

    #[test]
    fn message_get_negative_transfer_limit_skips_payload_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x77; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(-1i64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "negative transfer limit should behave as an impossible Python budget"
        );
    }

    #[test]
    fn message_get_string_transfer_limit_parses_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x88; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from("0"),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "string transfer limits should be parsed through Python float semantics"
        );
    }

    #[test]
    fn message_get_binary_string_transfer_limit_parses_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x99; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::Binary(b"0".to_vec()),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "binary string transfer limits should be parsed through Python float semantics"
        );
    }

    #[test]
    fn message_get_false_transfer_limit_skips_payload_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x9A; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::Boolean(false),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "False transfer limit should parse to Python float(False) == 0.0"
        );
    }

    #[test]
    fn message_get_true_transfer_limit_applies_one_kilobyte_budget_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x9B; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(&[0x42; 1_100]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::Boolean(true),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "True transfer limit should parse to Python float(True) == 1.0 KB"
        );
    }

    #[test]
    fn message_get_negative_infinity_transfer_limit_skips_payload_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x9C; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from("-inf"),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "negative infinity should preserve Python comparison semantics"
        );
    }
}
