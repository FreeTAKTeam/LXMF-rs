use super::*;
#[path = "inbound_control_peer.rs"]
mod peer_commands;
#[path = "inbound_control_propagation.rs"]
mod propagation_commands;
#[path = "inbound_control_response.rs"]
mod response;
#[path = "inbound_control_status.rs"]
mod status;
use response::ControlResponse;

pub(super) fn spawn_control_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
) {
    tokio::spawn(async move {
        let mut rx = transport.in_link_events();
        let identified = Arc::new(Mutex::new(HashMap::<AddressHash, Identity>::new()));
        loop {
            let Ok(event) = rx.recv().await else {
                break;
            };
            let LinkEvent::Data(payload) = event.event else {
                continue;
            };
            let destination_hex = hex::encode(event.address_hash.as_slice());
            let is_control_request =
                control.control_destination_hash_hex.as_deref() == Some(destination_hex.as_str());
            let is_propagation_request = control.propagation_destination_hash_hex.as_deref()
                == Some(destination_hex.as_str());
            if !is_control_request && !is_propagation_request {
                continue;
            }
            match payload.context() {
                PacketContext::LinkIdentify => {
                    if let Some(identity) =
                        parse_link_identify_payload(payload.as_slice(), &event.id)
                    {
                        if let Ok(mut guard) = identified.lock() {
                            guard.insert(event.id, identity);
                        }
                    }
                }
                PacketContext::Request => {
                    let Some(request_id) = payload.request_id() else {
                        continue;
                    };
                    let remote_identity =
                        identified.lock().ok().and_then(|guard| guard.get(&event.id).cloned());
                    let response = handle_control_request(
                        daemon.as_ref(),
                        &control,
                        payload.as_slice(),
                        remote_identity.as_ref(),
                        is_propagation_request,
                    );
                    let _ = response::send_control_response(
                        transport.as_ref(),
                        &event.id,
                        request_id,
                        response,
                    )
                    .await;
                }
                _ => {}
            }
        }
    });
}

fn parse_link_identify_payload(payload: &[u8], link_id: &AddressHash) -> Option<Identity> {
    if payload.len() < 32 + 32 + 64 {
        return None;
    }
    let identity = Identity::new_from_slices(&payload[..32], &payload[32..64]);
    let signature = ed25519_dalek::Signature::from_slice(&payload[64..128]).ok()?;
    let mut signed = Vec::with_capacity(16 + 64);
    signed.extend_from_slice(link_id.as_slice());
    signed.extend_from_slice(&payload[..64]);
    identity.verify(&signed, &signature).ok()?;
    Some(identity)
}

fn handle_control_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    payload: &[u8],
    remote_identity: Option<&Identity>,
    propagation_destination: bool,
) -> ControlResponse {
    const ERROR_NO_IDENTITY: u8 = 0xF0;
    const ERROR_NO_ACCESS: u8 = 0xF1;
    const ERROR_INVALID_KEY: u8 = 0xF3;
    const ERROR_INVALID_DATA: u8 = 0xF4;
    const ERROR_NOT_FOUND: u8 = 0xFD;

    if remote_identity.is_none() {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_IDENTITY);
    }
    let remote_identity = remote_identity.expect("checked above");
    let remote_hash = hex::encode(remote_identity.address_hash.as_slice());
    if !control_identity_allowed(control, &remote_hash) {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_ACCESS);
    }

    let Some((path_hash, data)) = parse_control_request_payload(payload) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
    };
    if propagation_destination {
        if path_hash == control_path_hash("/offer") {
            return propagation_commands::handle_offer_request(
                daemon,
                control,
                remote_identity,
                data,
                ERROR_INVALID_KEY,
                ERROR_INVALID_DATA,
            );
        }
        if path_hash == control_path_hash("/get") {
            return propagation_commands::handle_message_get_request(
                daemon,
                remote_identity,
                data,
                ERROR_INVALID_DATA,
            );
        }
        return ControlResponse::Code(ERROR_INVALID_DATA);
    }
    if path_hash == control_path_hash("/pn/get/stats") {
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

fn control_identity_allowed(control: &PropagationControlContext, remote_hash: &str) -> bool {
    if control.allowed_control_identities.is_empty() {
        return true;
    }
    control
        .allowed_control_identities
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(remote_hash))
}

fn parse_control_request_payload(payload: &[u8]) -> Option<([u8; 16], Option<rmpv::Value>)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(payload).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 3 {
        return None;
    }
    let path_bytes = match entries.get(1)? {
        rmpv::Value::Binary(bytes) if bytes.len() == 16 => bytes,
        _ => return None,
    };
    let mut path_hash = [0u8; 16];
    path_hash.copy_from_slice(path_bytes.as_slice());
    Some((path_hash, entries.get(2).cloned()))
}

fn control_path_hash(path: &str) -> [u8; 16] {
    let hash = rns_transport::hash::address_hash(path.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(hash.as_slice());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn python_status_uses_propagation_stamp_flexibility_not_delivery_stamp_flexibility() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "target_cost": 16,
                    "stamp_cost_flexibility": 7,
                    "peering_cost": 18,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "stamp_policy_set".to_string(),
                params: Some(json!({
                    "target_cost": 11,
                    "flexibility": 2,
                })),
            })
            .expect("set delivery stamp policy");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
            },
        );

        assert_eq!(status["stamp_cost_flexibility"].as_u64(), Some(7));
    }
}
