use super::*;
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

    let Some(peer_hex) = data.and_then(|value| match value {
        rmpv::Value::Binary(bytes) if bytes.len() == 16 => Some(hex::encode(bytes)),
        _ => None,
    }) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
    };
    let peer_exists = daemon
        .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.get("peers").cloned())
        .and_then(|value| value.as_array().cloned())
        .map(|rows| {
            rows.iter()
                .any(|row| row.get("peer").and_then(Value::as_str) == Some(peer_hex.as_str()))
        })
        .unwrap_or(false);
    if !peer_exists {
        return ControlResponse::Code(ERROR_NOT_FOUND);
    }

    if path_hash == control_path_hash("/pn/peer/sync") {
        let _ = daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "peer_sync".to_string(),
            params: Some(json!({ "peer": peer_hex })),
        });
        return ControlResponse::Bool(true);
    }
    if path_hash == control_path_hash("/pn/peer/unpeer") {
        let _ = daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "peer_unpeer".to_string(),
            params: Some(json!({ "peer": peer_hex })),
        });
        return ControlResponse::Bool(true);
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
    use reticulum_daemon::lxmf_stamps::generate_peering_key;
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

        let response = propagation_commands::handle_offer_request(
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
        let remote_delivery_hash =
            propagation_commands::delivery_destination_hash_for_identity(&remote_identity);
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

        let list_response = propagation_commands::handle_message_get_request(
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

        let fetch_response = propagation_commands::handle_message_get_request(
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
