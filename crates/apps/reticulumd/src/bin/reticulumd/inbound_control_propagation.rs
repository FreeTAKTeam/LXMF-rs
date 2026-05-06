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

pub(super) fn delivery_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    let name = DestinationName::new("lxmf", "delivery");
    let hash = sha2::Sha256::new()
        .chain_update(name.as_name_hash_slice())
        .chain_update(identity.address_hash.as_slice())
        .finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash[..16]);
    out
}
