use super::*;

pub fn handle_get(
    daemon: &impl GetBackend,
    remote_identity: &Identity,
    data: Option<rmpv::Value>,
    error_no_access: u8,
    error_invalid_data: u8,
) -> HandlerResponse {
    if !daemon.delivery_identity_allowed(remote_identity) {
        return HandlerResponse::Code(error_no_access);
    }
    let Some(rmpv::Value::Array(entries)) = data else {
        return HandlerResponse::Code(error_invalid_data);
    };
    if entries.len() < 2 {
        return HandlerResponse::Code(error_invalid_data);
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
                Err(_) => return HandlerResponse::Code(error_no_access),
            };
            if !completed {
                available.push((transient_id, size));
            }
        }
        if !available.is_empty()
            && daemon.record_propagation_offer_peer(remote_propagation_hash.as_str()).is_err()
        {
            return HandlerResponse::Code(error_no_access);
        }
        return HandlerResponse::Rmpv(rmpv::Value::Array(
            available
                .into_iter()
                .map(|(transient_id, _size)| rmpv::Value::Binary(transient_id))
                .collect(),
        ));
    }

    let haves = match entries.get(1) {
        Some(value) if value.is_nil() => Vec::new(),
        Some(rmpv::Value::Array(values)) => match validated_binary_id_list(values) {
            Some(ids) => ids,
            None => return HandlerResponse::Code(error_invalid_data),
        },
        _ => return HandlerResponse::Code(error_invalid_data),
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
            return HandlerResponse::Code(error_no_access);
        }
        if !daemon.retain_synced_on_node() {
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
                    Err(_) => return HandlerResponse::Code(error_no_access),
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
                return HandlerResponse::Code(error_no_access);
            }
        }
    }

    if entries.first().is_some_and(rmpv::Value::is_nil) {
        return HandlerResponse::Bool(true);
    }

    let wants = match entries.first() {
        Some(rmpv::Value::Array(values)) => match validated_binary_id_list(values) {
            Some(ids) => ids,
            None => return HandlerResponse::Code(error_invalid_data),
        },
        _ => return HandlerResponse::Code(error_invalid_data),
    };
    let mut retryable_wants = Vec::with_capacity(wants.len());
    for wanted in wants {
        let transient_id = hex::encode(wanted.as_slice());
        let completed = match daemon.has_peer_completed_propagation_mark(
            remote_propagation_hash.as_str(),
            transient_id.as_str(),
        ) {
            Ok(completed) => completed,
            Err(_) => return HandlerResponse::Code(error_no_access),
        };
        if !completed {
            retryable_wants.push(wanted);
        }
    }
    if retryable_wants.is_empty() {
        return HandlerResponse::Rmpv(rmpv::Value::Array(Vec::new()));
    }
    let transfer_limit_bytes = match entries.get(2) {
        None => None,
        Some(value) => match parse_transfer_limit_bytes(value) {
            Ok(limit) => limit,
            Err(err) => {
                log::warn!("[daemon] propagation transfer limit parse error: {err}");
                None
            }
        },
    };
    let preview = daemon.preview_propagation_payloads_for_destination_with_ids(
        &remote_delivery_hash,
        &retryable_wants,
        transfer_limit_bytes,
    );
    if !preview.is_empty()
        && daemon.record_propagation_offer_peer(remote_propagation_hash.as_str()).is_err()
    {
        return HandlerResponse::Code(error_no_access);
    }
    let fetched = daemon.fetch_propagation_payloads_for_destination_with_ids(
        &remote_delivery_hash,
        &retryable_wants,
        transfer_limit_bytes,
    );
    // Only authenticated receiver haves complete custody; constructing a response cannot.
    HandlerResponse::Rmpv(rmpv::Value::Array(
        fetched.into_iter().map(|(_transient_id, payload)| rmpv::Value::Binary(payload)).collect(),
    ))
}
