use response::ControlResponse;

#[cfg(test)]
use std::sync::Mutex;

pub(super) fn spawn_control_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
) {
    tokio::spawn(async move {
        let mut rx = transport.in_link_events();
        loop {
            let Ok(event) = rx.recv().await else {
                break;
            };
            let payload = match event.event {
                LinkEvent::Closed => {
                    clear_validated_peer_link(&control, &event.id);
                    continue;
                }
                LinkEvent::PeerIdentified(identity) => {
                    record_identified_peer(&control, event.id, *identity);
                    continue;
                }
                LinkEvent::Data(payload) => payload,
                _ => continue,
            };
            let destination_hex = hex::encode(event.address_hash.as_slice());
            let is_control_request =
                control.control_destination_hash_hex.as_deref() == Some(destination_hex.as_str());
            let is_propagation_request = control.propagation_destination_hash_hex.as_deref()
                == Some(destination_hex.as_str());
            log::debug!(
                "[daemon-control] link_data link={} destination={} context={:02x} propagation_destination={:?} control_destination={:?} is_propagation={} is_control={} len={}",
                event.id,
                destination_hex,
                payload.context() as u8,
                control.propagation_destination_hash_hex,
                control.control_destination_hash_hex,
                is_propagation_request,
                is_control_request,
                payload.len(),
            );
            if !is_control_request && !is_propagation_request {
                continue;
            }
            if payload.context() == PacketContext::Request {
                let Some(request_id) = payload.request_id() else {
                    continue;
                };
                let remote_identity = match control.identified_peer_links.lock() {
                    Ok(guard) => guard.get(&event.id).cloned(),
                    Err(err) => {
                        log::warn!("[daemon-control] failed to lock identified peer map: {err}");
                        None
                    }
                };
                let response = handle_control_request(
                    daemon.as_ref(),
                    &control,
                    &event.id,
                    payload.as_slice(),
                    remote_identity.as_ref(),
                    is_propagation_request,
                );
                if let Err(err) = response::send_control_response(
                    transport.as_ref(),
                    &event.id,
                    request_id,
                    response,
                )
                .await
                {
                    log::error!(
                        "[daemon-control] failed to send response link={} propagation_request={} error={}",
                        event.id,
                        is_propagation_request,
                        err
                    );
                }
            }
        }
    });
}

fn record_identified_peer(
    control: &PropagationControlContext,
    link_id: AddressHash,
    identity: Identity,
) {
    match control.identified_peer_links.lock() {
        Ok(mut guard) => {
            guard.insert(link_id, identity);
        }
        Err(err) => {
            log::warn!("[daemon-control] failed to lock identified peer map: {err}");
        }
    }
}

fn clear_validated_peer_link(control: &PropagationControlContext, link_id: &AddressHash) {
    if let Ok(mut guard) = control.validated_peer_links.lock() {
        guard.remove(link_id);
    }
    if let Ok(mut guard) = control.identified_peer_links.lock() {
        guard.remove(link_id);
    }
}

fn handle_control_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    link_id: &AddressHash,
    payload: &[u8],
    remote_identity: Option<&Identity>,
    propagation_destination: bool,
) -> ControlResponse {
    const ERROR_NO_IDENTITY: u8 = 0xF0;
    const ERROR_NO_ACCESS: u8 = 0xF1;
    const ERROR_INVALID_KEY: u8 = 0xF3;
    const ERROR_INVALID_DATA: u8 = 0xF4;
    const ERROR_THROTTLED: u8 = 0xF6;
    const ERROR_NOT_FOUND: u8 = 0xFD;

    if remote_identity.is_none() {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_IDENTITY);
    }
    let remote_identity = remote_identity.expect("checked above");
    let remote_hash = hex::encode(remote_identity.address_hash.as_slice());
    if !propagation_destination && !control_identity_allowed(control, &remote_hash) {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_ACCESS);
    }

    let (path_hash, data) = match parse_control_request_payload(payload) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("[daemon-control] failed to parse control request payload: {err}");
            return ControlResponse::Code(ERROR_INVALID_DATA);
        }
    };
    if propagation_destination {
        if path_hash == control_path_hash("/offer") {
            return propagation_commands::handle_offer_request(
                daemon,
                control,
                link_id,
                remote_identity,
                data,
                ERROR_NO_ACCESS,
                ERROR_INVALID_KEY,
                ERROR_INVALID_DATA,
                ERROR_THROTTLED,
            );
        }
        if path_hash == control_path_hash("/get") {
            return propagation_commands::handle_message_get_request(
                daemon,
                remote_identity,
                data,
                ERROR_NO_ACCESS,
                ERROR_INVALID_DATA,
            );
        }
        return ControlResponse::Code(ERROR_INVALID_DATA);
    }
    if path_hash == control_path_hash("/pn/get/stats") {
        if !daemon.current_propagation_state().enabled {
            return ControlResponse::Value(Value::Null);
        }
        return ControlResponse::Value(status::compose_python_status(daemon, control));
    }
    if let Some(response) = peer_commands::handle_peer_command(
        daemon,
        path_hash,
        data,
        ERROR_INVALID_DATA,
        ERROR_NOT_FOUND,
    ) {
        return response;
    }

    ControlResponse::Code(ERROR_INVALID_DATA)
}

fn resource_control_response(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    link_id: &AddressHash,
    payload: &[u8],
    remote_identity: Option<&Identity>,
    propagation_destination: bool,
) -> ControlResponse {
    handle_control_request(
        daemon,
        control,
        link_id,
        payload,
        remote_identity,
        propagation_destination,
    )
}

pub(super) async fn handle_resource_control_request(
    daemon: &RpcDaemon,
    transport: &Transport,
    control: &PropagationControlContext,
    link_id: &AddressHash,
    payload: &[u8],
    request_id: [u8; 16],
    propagation_destination: bool,
) -> Result<(), std::io::Error> {
    let remote_identity = remote_identity_for_resource_link(control, transport, link_id).await;
    let response = resource_control_response(
        daemon,
        control,
        link_id,
        payload,
        remote_identity.as_ref(),
        propagation_destination,
    );
    response::send_control_response(transport, link_id, request_id, response).await
}

async fn remote_identity_for_resource_link(
    control: &PropagationControlContext,
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<Identity> {
    match control.identified_peer_links.lock() {
        Ok(guard) => {
            if let Some(identity) = guard.get(link_id) {
                return Some(*identity);
            }
        }
        Err(err) => {
            log::warn!("[daemon-control] failed to lock identified peer map: {err}");
        }
    }
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        return Some(*guard.identified_peer_identity().unwrap_or_else(|| guard.peer_identity()));
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        return Some(*guard.identified_peer_identity().unwrap_or_else(|| guard.peer_identity()));
    }
    None
}

fn control_identity_allowed(control: &PropagationControlContext, remote_hash: &str) -> bool {
    if control.allowed_control_identities.is_empty() {
        return true;
    }
    control
        .allowed_control_identities
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(remote_hash))
}

fn parse_control_request_payload(
    payload: &[u8],
) -> Result<([u8; 16], Option<rmpv::Value>), String> {
    let value = rmp_serde::from_slice::<rmpv::Value>(payload).map_err(|e| e.to_string())?;
    let rmpv::Value::Array(entries) = value else {
        return Err("control request payload is not an array".to_string());
    };
    if entries.len() != 3 {
        return Err(format!(
            "control request payload array has {} entries, expected 3",
            entries.len()
        ));
    }
    let path_bytes = match entries.get(1) {
        Some(rmpv::Value::Binary(bytes)) if bytes.len() == 16 => bytes,
        _ => return Err("control request path hash missing or wrong length".to_string()),
    };
    let mut path_hash = [0u8; 16];
    path_hash.copy_from_slice(path_bytes.as_slice());
    Ok((path_hash, entries.get(2).cloned()))
}

fn control_path_hash(path: &str) -> [u8; 16] {
    let hash = rns_transport::hash::address_hash(path.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(hash.as_slice());
    out
}
