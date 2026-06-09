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
use std::collections::HashMap;
use std::sync::Mutex;

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
            let payload = match event.event {
                LinkEvent::Closed => {
                    clear_validated_peer_link(&control, &event.id);
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
            if std::env::var("RETICULUMD_DIAGNOSTICS").ok().is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            }) {
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
            }
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
                _ => {}
            }
        }
    });
}

fn clear_validated_peer_link(control: &PropagationControlContext, link_id: &AddressHash) {
    if let Ok(mut guard) = control.validated_peer_links.lock() {
        guard.remove(link_id);
    }
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

    let Some((path_hash, data)) = parse_control_request_payload(payload) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
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
    use rns_rpc::MessagesStore;
    use serde_json::json;
    use std::collections::HashSet;

    fn test_validated_peer_links() -> Arc<Mutex<HashSet<AddressHash>>> {
        Arc::new(Mutex::new(HashSet::new()))
    }

    fn test_link_id() -> AddressHash {
        AddressHash::new([0xA5; 16])
    }

    fn test_control_context() -> PropagationControlContext {
        PropagationControlContext {
            enabled: true,
            local_identity_hash: [0u8; 16],
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        }
    }

    fn ready_propagation_daemon() -> RpcDaemon {
        RpcDaemon::test_instance_with_identity(hex::encode([2u8; 16]))
    }

    fn make_ready_propagation_peer(daemon: &RpcDaemon, peer_seed: u8) -> String {
        let peer = hex::encode([peer_seed; 16]);
        daemon
            .accept_announce_with_metadata(
                peer.clone(),
                1_700_000_606 + i64::from(peer_seed),
                None,
                None,
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(1)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept ready propagation peer announce");
        peer
    }

    fn control_request(path: &str, data: rmpv::Value) -> Vec<u8> {
        rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
            rmpv::Value::Nil,
            rmpv::Value::Binary(control_path_hash(path).to_vec()),
            data,
        ]))
        .expect("encode control request")
    }

    #[test]
    fn closed_link_clears_validated_peer_link_like_python() {
        let control = test_control_context();
        let link_id = test_link_id();
        control.validated_peer_links.lock().expect("validated peer links").insert(link_id);

        clear_validated_peer_link(&control, &link_id);

        assert!(!control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
    }

    #[test]
    fn stats_request_returns_nil_when_propagation_node_is_disabled() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        let response = handle_control_request(
            &daemon,
            &test_control_context(),
            &test_link_id(),
            control_request("/pn/get/stats", rmpv::Value::Nil).as_slice(),
            Some(&remote_identity),
            false,
        );

        assert!(matches!(response, ControlResponse::Value(Value::Null)));
    }

    #[test]
    fn stats_request_returns_status_when_propagation_node_is_enabled() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({ "enabled": true })),
            })
            .expect("enable propagation");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        let response = handle_control_request(
            &daemon,
            &test_control_context(),
            &test_link_id(),
            control_request("/pn/get/stats", rmpv::Value::Nil).as_slice(),
            Some(&remote_identity),
            false,
        );

        let ControlResponse::Value(status) = response else {
            panic!("expected status value");
        };
        assert_eq!(status["peers"].as_object().map(|peers| peers.len()), Some(0));
        assert_eq!(status["total_peers"].as_u64(), Some(0));
    }

    #[test]
    fn stats_request_rejects_identity_outside_control_allow_list() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({ "enabled": true })),
            })
            .expect("enable propagation");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let mut control = test_control_context();
        control.allowed_control_identities = vec!["not-the-remote".to_string()];

        let response = handle_control_request(
            &daemon,
            &control,
            &test_link_id(),
            control_request("/pn/get/stats", rmpv::Value::Nil).as_slice(),
            Some(&remote_identity),
            false,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn propagation_offer_ignores_control_allow_list_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
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
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: vec!["not-the-remote".to_string()],
            validated_peer_links: test_validated_peer_links(),
        };
        let response = handle_control_request(
            &daemon,
            &control,
            &test_link_id(),
            control_request(
                "/offer",
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(peering_key),
                    rmpv::Value::Array(Vec::new()),
                ]),
            )
            .as_slice(),
            Some(&remote_identity),
            true,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
    }

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
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["stamp_cost_flexibility"].as_u64(), Some(7));
    }

    #[test]
    fn python_status_reports_elapsed_uptime_not_epoch_time() {
        let daemon = RpcDaemon::test_instance();

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert!(
            status["uptime"].as_u64().is_some_and(|value| value < 60),
            "uptime should be elapsed seconds, not Unix epoch seconds"
        );
    }

    #[test]
    fn python_status_uses_configured_node_transfer_limits() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "delivery_limit": 321,
                    "propagation_limit": 654,
                    "sync_limit": 987,
                })),
            })
            .expect("enable propagation");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["delivery_limit"].as_u64(), Some(321));
        assert_eq!(status["propagation_limit"].as_u64(), Some(654));
        assert_eq!(status["sync_limit"].as_u64(), Some(987));
    }

    #[test]
    fn python_status_reports_message_storage_limit_in_decimal_bytes() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "message_storage_limit_mb": 4,
                })),
            })
            .expect("enable propagation");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["messagestore"]["limit"].as_u64(), Some(4_000_000));
    }

    #[test]
    fn python_status_uses_zero_acceptance_rate_before_offers() {
        let peer = "peer-zero-acceptance".to_string();
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["peers"][peer.as_str()]["acceptance_rate"].as_f64(), Some(0.0));
    }

    #[test]
    fn python_status_reports_peer_sync_transfer_rate_counter() {
        let daemon = ready_propagation_daemon();
        let peer = make_ready_propagation_peer(&daemon, 0x91);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({ "enabled": true })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": "19".repeat(24) })),
            })
            .expect("ingest propagation");
        let sync = daemon
            .handle_rpc(RpcRequest {
                id: 3,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer, "transfer_limit_kb": 1 })),
            })
            .expect("peer sync")
            .result
            .expect("peer sync result");
        let transferred_bytes =
            sync["sync_transfer_rate"].as_f64().expect("sync transfer rate counter") as u64;
        assert!(transferred_bytes > 0);

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["sync_transfer_rate"].as_f64(), Some(transferred_bytes as f64));
        assert_eq!(peer_status["str"].as_u64(), Some(transferred_bytes));
    }

    #[test]
    fn python_status_reports_peer_propagation_message_ids() {
        let daemon = ready_propagation_daemon();
        let peer = make_ready_propagation_peer(&daemon, 0x92);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");
        let handled_id = "8a".repeat(32);
        let unhandled_id = "8b".repeat(32);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                b"handled propagation payload",
                handled_id.as_str(),
                &[],
            )
            .expect("store handled payload");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer, "transfer_limit_kb": 1 })),
            })
            .expect("handle first payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                b"unhandled propagation payload",
                unhandled_id.as_str(),
                &[],
            )
            .expect("store unhandled payload");
        daemon.record_propagation_offer_peer(peer.as_str()).expect("record offered peer");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(
            peer_status["messages"]["handled_ids"].as_array().expect("message handled ids"),
            &[json!(handled_id.as_str())]
        );
        assert_eq!(
            peer_status["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
            &[json!(unhandled_id.as_str())]
        );
        assert_eq!(
            peer_status["handled_ids"].as_array().expect("top-level handled ids"),
            &[json!(handled_id.as_str())]
        );
        assert_eq!(
            peer_status["unhandled_ids"].as_array().expect("top-level unhandled ids"),
            &[json!(unhandled_id.as_str())]
        );
    }

    #[test]
    fn python_status_reports_peer_message_counters_at_top_level() {
        let daemon = ready_propagation_daemon();
        let peer = make_ready_propagation_peer(&daemon, 0x93);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");
        let handled_id = "8c".repeat(32);
        let handled_payload = [0x14; 32];
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                handled_payload.as_slice(),
                handled_id.as_str(),
                &[],
            )
            .expect("store handled propagation payload");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer, "transfer_limit_kb": 1 })),
            })
            .expect("handle first payload");
        let unhandled_id = "8d".repeat(32);
        let unhandled_payload = [0x15; 32];
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                unhandled_payload.as_slice(),
                unhandled_id.as_str(),
                &[],
            )
            .expect("store unhandled propagation payload");
        daemon.record_propagation_offer_peer(peer.as_str()).expect("record offered peer");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["messages"]["offered"].as_u64(), Some(1));
        assert_eq!(peer_status["messages"]["unhandled"].as_u64(), Some(1));
        assert_eq!(peer_status["messages"]["offered_bytes"].as_u64(), Some(32));
        assert_eq!(peer_status["messages"]["unhandled_bytes"].as_u64(), Some(32));
        assert_eq!(peer_status["offered"].as_u64(), Some(1));
        assert_eq!(peer_status["outgoing"].as_u64(), Some(1));
        assert_eq!(peer_status["incoming"].as_u64(), Some(0));
        assert_eq!(peer_status["unhandled"].as_u64(), Some(1));
        assert_eq!(peer_status["offered_bytes"].as_u64(), Some(32));
        assert_eq!(peer_status["unhandled_bytes"].as_u64(), Some(32));
    }

    #[test]
    fn python_status_reports_peer_record_metadata() {
        let peer = "peer-record-metadata".to_string();
        let daemon = RpcDaemon::test_instance();
        let sync = daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync")
            .result
            .expect("peer sync result");
        let first_seen = sync["first_seen"].as_i64().expect("first_seen");
        assert!(first_seen > 0);

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["peer_type"].as_str(), Some("manual"));
        assert_eq!(peer_status["first_seen"].as_i64(), Some(first_seen));
        assert_eq!(peer_status["seen_count"].as_u64(), Some(1));
        assert_eq!(peer_status["sync_strategy"].as_u64(), Some(2));
    }

    #[test]
    fn python_status_reports_propagation_node_runtime_state() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": ["peer-selected-node"],
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "set_outbound_propagation_node".to_string(),
                params: Some(json!({ "peer": "peer-selected-node" })),
            })
            .expect("set selected node");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["selected_node"].as_str(), Some("peer-selected-node"));
        assert_eq!(status["sync_state"].as_u64(), Some(0));
        assert_eq!(status["sync_progress"].as_f64(), Some(0.0));
        assert_eq!(status["last_sync_started"], Value::Null);
        assert_eq!(status["last_sync_completed"], Value::Null);
        assert_eq!(status["last_sync_error"], Value::Null);
    }

    #[test]
    fn python_status_reports_propagation_policy_and_ingest_counters() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "autopeer": false,
                    "autopeer_maxdepth": 2,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": "2a".repeat(24) })),
            })
            .expect("ingest propagation");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["autopeer"].as_bool(), Some(false));
        assert_eq!(status["autopeer_maxdepth"].as_u64(), Some(2));
        assert_eq!(status["total_ingested"].as_u64(), Some(1));
        assert_eq!(status["last_ingest_count"].as_u64(), Some(1));
        assert_eq!(status["messages_received"].as_u64(), Some(0));
        assert_eq!(status["max_messages"].as_u64(), Some(0));
    }

    #[test]
    fn python_status_preserves_unknown_peer_propagation_policy_as_null() {
        let peer = "peer-unknown-policy".to_string();
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
                    "propagation_limit": 654,
                    "sync_limit": 987,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["transfer_limit"], Value::Null);
        assert_eq!(peer_status["sync_limit"], Value::Null);
        assert_eq!(peer_status["target_stamp_cost"], Value::Null);
        assert_eq!(peer_status["stamp_cost_flexibility"], Value::Null);
        assert_eq!(peer_status["peering_cost"], Value::Null);
    }

    #[test]
    fn python_status_collapses_internal_peer_types_to_static_or_discovered() {
        let static_peer = "peer-static".to_string();
        let auto_peer = "peer-auto".to_string();
        let manual_peer = "peer-manual".to_string();
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": [static_peer],
                    "autopeer": true,
                })),
            })
            .expect("enable propagation");
        daemon
            .accept_announce_with_metadata(
                auto_peer.clone(),
                1_700_000_800,
                None,
                None,
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(1)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept auto peer announce");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": manual_peer })),
            })
            .expect("manual peer sync");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["peers"][static_peer.as_str()]["type"].as_str(), Some("static"));
        assert_eq!(status["peers"][auto_peer.as_str()]["type"].as_str(), Some("discovered"));
        assert_eq!(status["peers"][manual_peer.as_str()]["type"].as_str(), Some("discovered"));
        assert_eq!(status["static_peers"].as_u64(), Some(1));
        assert_eq!(status["discovered_peers"].as_u64(), Some(2));
        assert_eq!(status["total_peers"].as_u64(), Some(3));
    }

    #[test]
    fn python_status_exposes_peer_peering_key_value() {
        let local_hash = [2u8; 16];
        let peer = hex::encode([3u8; 16]);
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode(local_hash),
        );
        daemon
            .accept_announce_with_metadata(
                peer.clone(),
                1_700_000_620,
                None,
                None,
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(1)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept propagation peer announce");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: local_hash,
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert!(status["peers"][peer.as_str()]["peering_key"]
            .as_u64()
            .is_some_and(|value| value >= 1));
    }

    #[test]
    fn python_status_exposes_peer_peering_key_status() {
        let local_hash = [2u8; 16];
        let ready_peer = hex::encode([3u8; 16]);
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode(local_hash),
        );
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-unconfigured-key" })),
            })
            .expect("create unconfigured peer");
        daemon
            .accept_announce_with_metadata(
                ready_peer.clone(),
                1_700_000_620,
                None,
                None,
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(1)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept propagation peer announce");
        daemon
            .accept_announce_with_metadata(
                "peer-not-ready-key".to_string(),
                1_700_000_621,
                None,
                None,
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(1)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept invalid-hash propagation peer announce");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: local_hash,
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(
            status["peers"]["peer-unconfigured-key"]["peering_key_status"].as_str(),
            Some("unconfigured")
        );
        assert_eq!(
            status["peers"][ready_peer.as_str()]["peering_key_status"].as_str(),
            Some("ready")
        );
        assert_eq!(
            status["peers"]["peer-not-ready-key"]["peering_key_status"].as_str(),
            Some("not_ready")
        );
    }

    #[test]
    fn python_status_prefers_peer_propagation_stamp_policy() {
        let peer = "peer-policy".to_string();
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "autopeer": true,
                    "target_cost": 16,
                    "stamp_cost_flexibility": 7,
                    "peering_cost": 18,
                })),
            })
            .expect("enable propagation");
        let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
            rmpv::Value::Boolean(false),
            rmpv::Value::from(1_700_000_700),
            rmpv::Value::Boolean(true),
            rmpv::Value::from(512),
            rmpv::Value::from(2048),
            rmpv::Value::Array(vec![
                rmpv::Value::from(4),
                rmpv::Value::from(1),
                rmpv::Value::from(6),
            ]),
            rmpv::Value::Map(Vec::new()),
        ]))
        .expect("encode propagation app data");
        daemon
            .accept_announce_with_metadata(
                peer.clone(),
                1_700_000_700,
                Some("Peer Policy".to_string()),
                Some("announce".to_string()),
                Some(hex::encode(app_data)),
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(4),
                Some(Some(1)),
                Some(Some(6)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept propagation peer announce");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["peering_timebase"].as_i64(), Some(1_700_000_700));
        assert_eq!(peer_status["transfer_limit"].as_u64(), Some(512));
        assert_eq!(peer_status["sync_limit"].as_u64(), Some(2048));
        assert_eq!(peer_status["target_stamp_cost"].as_u64(), Some(4));
        assert_eq!(peer_status["stamp_cost_flexibility"].as_u64(), Some(1));
        assert_eq!(peer_status["peering_cost"].as_u64(), Some(6));
    }
}
