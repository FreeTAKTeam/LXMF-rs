use super::*;
use reticulum_daemon::lxmf_stamps::validate_peering_key;
use rns_transport::destination::DestinationName;
use sha2::Digest;

pub(super) fn handle_message_get_request(
    daemon: &RpcDaemon,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_invalid_data: u8,
) -> ControlResponse {
    let Some(rmpv::Value::Array(entries)) = data else {
        return ControlResponse::Code(error_invalid_data);
    };
    if entries.len() < 2 {
        return ControlResponse::Code(error_invalid_data);
    }
    let remote_delivery_hash = delivery_destination_hash_for_identity(remote_identity);
    if entries.first().is_some_and(rmpv::Value::is_nil)
        && entries.get(1).is_some_and(rmpv::Value::is_nil)
    {
        return ControlResponse::Rmpv(rmpv::Value::Array(
            daemon
                .list_propagation_payloads_for_destination(&remote_delivery_hash)
                .into_iter()
                .map(|(transient_id, _size)| rmpv::Value::Binary(transient_id))
                .collect(),
        ));
    }

    let haves = match entries.get(1) {
        Some(value) if value.is_nil() => Vec::new(),
        Some(rmpv::Value::Array(values)) => match binary_id_list(values) {
            Some(ids) => ids,
            None => return ControlResponse::Code(error_invalid_data),
        },
        _ => return ControlResponse::Code(error_invalid_data),
    };
    if !haves.is_empty() {
        daemon.purge_propagation_payloads_for_destination(&remote_delivery_hash, &haves);
    }

    let wants = match entries.first() {
        Some(value) if value.is_nil() => Vec::new(),
        Some(rmpv::Value::Array(values)) => match binary_id_list(values) {
            Some(ids) => ids,
            None => return ControlResponse::Code(error_invalid_data),
        },
        _ => return ControlResponse::Code(error_invalid_data),
    };
    if wants.is_empty() {
        return ControlResponse::Rmpv(rmpv::Value::Array(Vec::new()));
    }
    let transfer_limit_bytes = entries.get(2).and_then(parse_transfer_limit_bytes);
    ControlResponse::Rmpv(rmpv::Value::Array(
        daemon
            .fetch_propagation_payloads_for_destination(
                &remote_delivery_hash,
                &wants,
                transfer_limit_bytes,
            )
            .into_iter()
            .map(rmpv::Value::Binary)
            .collect(),
    ))
}

pub(super) fn handle_offer_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_invalid_key: u8,
    error_invalid_data: u8,
) -> ControlResponse {
    let Some(rmpv::Value::Array(entries)) = data else {
        return ControlResponse::Code(error_invalid_data);
    };
    if entries.len() < 2 {
        return ControlResponse::Code(error_invalid_data);
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

    let mut wanted = Vec::new();
    for transient_id in transient_ids {
        let rmpv::Value::Binary(bytes) = transient_id else {
            return ControlResponse::Code(error_invalid_data);
        };
        if bytes.len() != 32 {
            return ControlResponse::Code(error_invalid_data);
        }
        let transient_hex = hex::encode(bytes);
        if !daemon.has_propagation_payload(transient_hex.as_str()) {
            wanted.push(bytes.clone());
        }
    }

    if wanted.is_empty() {
        ControlResponse::Bool(false)
    } else if wanted.len() == transient_ids.len() {
        ControlResponse::Bool(true)
    } else {
        ControlResponse::Rmpv(rmpv::Value::Array(
            wanted.into_iter().map(rmpv::Value::Binary).collect(),
        ))
    }
}

fn binary_id_list(values: &[rmpv::Value]) -> Option<Vec<Vec<u8>>> {
    values
        .iter()
        .map(|value| match value {
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
        _ => None,
    }?;
    (limit.is_finite() && limit > 0.0).then_some((limit * 1000.0) as usize)
}

fn delivery_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    let name = DestinationName::new("lxmf", "delivery");
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
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(existing.to_vec()),
                    rmpv::Value::Binary(missing.to_vec()),
                ]),
            ])),
            0xF3,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(wanted)) = response else {
            panic!("expected partial wanted-id list");
        };
        assert_eq!(wanted, vec![rmpv::Value::Binary(missing.to_vec())]);
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
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
        assert!(!daemon.has_propagation_payload(hex::encode(have).as_str()));
        assert!(daemon.has_propagation_payload(hex::encode(ignored).as_str()));
    }
}
