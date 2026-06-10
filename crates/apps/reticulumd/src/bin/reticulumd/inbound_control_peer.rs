use super::*;

pub(super) fn handle_peer_command(
    daemon: &RpcDaemon,
    path_hash: [u8; 16],
    data: Option<rmpv::Value>,
    error_invalid_data: u8,
    error_not_found: u8,
) -> Option<ControlResponse> {
    let sync_command = path_hash == control_path_hash("/pn/peer/sync");
    let method = if sync_command {
        "peer_sync"
    } else if path_hash == control_path_hash("/pn/peer/unpeer") {
        "peer_unpeer"
    } else {
        return None;
    };
    let Some((peer_hex, transfer_limit_kb)) = peer_request_from_data(data) else {
        return Some(ControlResponse::Code(error_invalid_data));
    };
    if !peer_exists(daemon, peer_hex.as_str()) {
        return Some(ControlResponse::Code(error_not_found));
    }
    let mut params = json!({ "peer": peer_hex });
    if sync_command {
        params["force_sync"] = json!(true);
    }
    if let Some(transfer_limit_kb) = transfer_limit_kb {
        params["transfer_limit_kb"] = json!(transfer_limit_kb);
    }
    let result =
        daemon.handle_rpc(RpcRequest { id: 0, method: method.to_string(), params: Some(params) });
    result
        .ok()
        .and_then(|response| response.result)
        .map(ControlResponse::Value)
        .or(Some(ControlResponse::Code(error_invalid_data)))
}

fn peer_request_from_data(data: Option<rmpv::Value>) -> Option<(String, Option<f64>)> {
    match data {
        Some(rmpv::Value::Binary(bytes)) if bytes.len() == 16 => Some((hex::encode(bytes), None)),
        Some(rmpv::Value::Array(entries)) => {
            let peer = match entries.first()? {
                rmpv::Value::Binary(bytes) if bytes.len() == 16 => hex::encode(bytes),
                _ => return None,
            };
            let transfer_limit_kb = match entries.get(1) {
                Some(value) => transfer_limit_kb_from_value(value)?,
                None => None,
            };
            Some((peer, transfer_limit_kb))
        }
        _ => None,
    }
}

fn transfer_limit_kb_from_value(value: &rmpv::Value) -> Option<Option<f64>> {
    let limit = match value {
        rmpv::Value::F64(value) => Some(*value),
        rmpv::Value::F32(value) => Some((*value).into()),
        rmpv::Value::Integer(value) => value.as_f64(),
        rmpv::Value::String(value) => value.as_str()?.trim().parse::<f64>().ok(),
        rmpv::Value::Binary(value) => std::str::from_utf8(value).ok()?.trim().parse::<f64>().ok(),
        rmpv::Value::Boolean(value) => Some(f64::from(*value as u8)),
        _ => None,
    }?;
    if limit.is_nan() {
        None
    } else if limit.is_infinite() && limit.is_sign_positive() {
        Some(None)
    } else {
        Some(Some(limit.max(0.0)))
    }
}

fn peer_exists(daemon: &RpcDaemon, peer_hex: &str) -> bool {
    daemon
        .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.get("peers").cloned())
        .and_then(|value| value.as_array().cloned())
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("peer")
                    .and_then(Value::as_str)
                    .is_some_and(|peer| peer.eq_ignore_ascii_case(peer_hex))
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_rpc::MessagesStore;

    const ERROR_INVALID_DATA: u8 = 0xF4;
    const ERROR_NOT_FOUND: u8 = 0xFD;

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

    #[test]
    fn peer_command_returns_none_for_unhandled_path() {
        let daemon = RpcDaemon::test_instance();

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/get/stats"),
            Some(rmpv::Value::Binary(vec![0; 16])),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        );

        assert!(response.is_none());
    }

    #[test]
    fn peer_command_returns_not_found_for_unknown_peer() {
        let daemon = RpcDaemon::test_instance();

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/sync"),
            Some(rmpv::Value::Binary(vec![0xA5; 16])),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        );

        assert!(matches!(response, Some(ControlResponse::Code(ERROR_NOT_FOUND))));
    }

    #[test]
    fn peer_request_accepts_transfer_limit_array_payload() {
        let peer_bytes = [0xA5; 16];

        let (peer_hex, transfer_limit_kb) = peer_request_from_data(Some(rmpv::Value::Array(vec![
            rmpv::Value::Binary(peer_bytes.to_vec()),
            rmpv::Value::F64(42.5),
        ])))
        .expect("peer request");

        assert_eq!(peer_hex, hex::encode(peer_bytes));
        assert_eq!(transfer_limit_kb, Some(42.5));
    }

    #[test]
    fn peer_request_rejects_invalid_transfer_limit_array_payload() {
        let peer_bytes = [0xA5; 16];

        let request = peer_request_from_data(Some(rmpv::Value::Array(vec![
            rmpv::Value::Binary(peer_bytes.to_vec()),
            rmpv::Value::Nil,
        ])));

        assert!(request.is_none());
    }

    #[test]
    fn peer_request_positive_infinity_transfer_limit_omits_override() {
        let peer_bytes = [0xA5; 16];

        let (peer_hex, transfer_limit_kb) = peer_request_from_data(Some(rmpv::Value::Array(vec![
            rmpv::Value::Binary(peer_bytes.to_vec()),
            rmpv::Value::String("inf".into()),
        ])))
        .expect("peer request");

        assert_eq!(peer_hex, hex::encode(peer_bytes));
        assert_eq!(transfer_limit_kb, None);
    }

    #[test]
    fn peer_request_negative_infinity_transfer_limit_clamps_to_zero() {
        let peer_bytes = [0xA5; 16];

        let (peer_hex, transfer_limit_kb) = peer_request_from_data(Some(rmpv::Value::Array(vec![
            rmpv::Value::Binary(peer_bytes.to_vec()),
            rmpv::Value::String("-inf".into()),
        ])))
        .expect("peer request");

        assert_eq!(peer_hex, hex::encode(peer_bytes));
        assert_eq!(transfer_limit_kb, Some(0.0));
    }

    #[test]
    fn peer_sync_command_returns_daemon_sync_result() {
        let peer_bytes = [0xC7; 16];
        let peer_hex = hex::encode(peer_bytes);
        let payload_hex = format!("{}{}", "23".repeat(16), "45".repeat(24));
        let daemon = RpcDaemon::test_instance_with_identity(hex::encode([2u8; 16]));
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": [peer_hex],
                })),
            })
            .expect("enable propagation");
        assert_eq!(make_ready_propagation_peer(&daemon, 0xC7), peer_hex);
        let ingest = daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": payload_hex })),
            })
            .expect("ingest propagation")
            .result
            .expect("ingest result");
        let transient_id = ingest["transient_id"].as_str().expect("transient id");

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/sync"),
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peer_bytes.to_vec()),
                rmpv::Value::F64(1.0),
            ])),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        )
        .expect("peer sync command response");

        let ControlResponse::Value(result) = response else {
            panic!("expected peer sync result value");
        };
        assert_eq!(result["peer"].as_str(), Some(peer_hex.as_str()));
        assert_eq!(result["synced"].as_bool(), Some(true));
        assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
        assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
        assert_eq!(result["propagation"]["transferred_ids"], json!([transient_id]));
        assert_eq!(result["propagation"]["messages"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn peer_sync_command_bypasses_existing_backoff_like_python() {
        let peer_bytes = [0xC6; 16];
        let peer_hex = hex::encode(peer_bytes);
        let payload_hex = format!("{}{}", "23".repeat(16), "46".repeat(24));
        let daemon = RpcDaemon::test_instance_with_identity(hex::encode([2u8; 16]));
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": [peer_hex],
                })),
            })
            .expect("enable propagation");
        assert_eq!(make_ready_propagation_peer(&daemon, 0xC6), peer_hex);
        let ingest = daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": payload_hex })),
            })
            .expect("ingest propagation")
            .result
            .expect("ingest result");
        let transient_id = ingest["transient_id"].as_str().expect("transient id");
        daemon.record_outbound_peer_activity(peer_hex.as_str(), 64, false);
        let peers = daemon
            .handle_rpc(RpcRequest { id: 3, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(peer_hex.as_str()))
            .expect("peer row");
        assert!(row["sync_backoff"].as_u64().expect("sync backoff") > 0);
        assert!(row["next_sync_attempt"].as_i64().expect("next sync attempt") > 0);

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/sync"),
            Some(rmpv::Value::Binary(peer_bytes.to_vec())),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        )
        .expect("peer sync command response");

        let ControlResponse::Value(result) = response else {
            panic!("expected peer sync result value");
        };
        assert_eq!(result["peer"].as_str(), Some(peer_hex.as_str()));
        assert_eq!(result["synced"].as_bool(), Some(true));
        assert_ne!(result["postpone_reason"].as_str(), Some("backoff"));
        assert_eq!(result["propagation"]["transferred_ids"], json!([transient_id]));
        assert_eq!(result["sync_backoff"].as_u64(), Some(0));
        assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    }

    #[test]
    fn peer_sync_command_reports_peering_key_status() {
        let peer_bytes = [0xC8; 16];
        let peer_hex = hex::encode(peer_bytes);
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode([2u8; 16]),
        );
        daemon
            .accept_announce_with_metadata(
                peer_hex.clone(),
                1_700_000_611,
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

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/sync"),
            Some(rmpv::Value::Binary(peer_bytes.to_vec())),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        )
        .expect("peer sync command response");

        let ControlResponse::Value(result) = response else {
            panic!("expected peer sync result value");
        };
        assert_eq!(result["peer"].as_str(), Some(peer_hex.as_str()));
        assert_eq!(result["peering_key_status"].as_str(), Some("ready"));
        assert_eq!(result["propagation"]["peering_key_status"].as_str(), Some("ready"));
    }

    #[test]
    fn peer_sync_command_accepts_case_variant_existing_peer_like_python() {
        let peer_bytes = [0xCA; 16];
        let stored_peer = hex::encode(peer_bytes).to_ascii_uppercase();
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode([2u8; 16]),
        );
        daemon
            .accept_announce_with_metadata(
                stored_peer.clone(),
                1_700_000_612,
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
            .expect("accept mixed-case propagation peer announce");

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/sync"),
            Some(rmpv::Value::Binary(peer_bytes.to_vec())),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        )
        .expect("peer sync command response");

        let ControlResponse::Value(result) = response else {
            panic!("expected peer sync result value");
        };
        assert_eq!(result["peer"].as_str(), Some(stored_peer.as_str()));
        assert_ne!(result["error"].as_str(), Some("not_found"));
    }

    #[test]
    fn peer_unpeer_command_returns_daemon_cleanup_result() {
        let daemon = RpcDaemon::test_instance();
        let peer_bytes = [0xB6; 16];
        let peer_hex = hex::encode(peer_bytes);
        let payload_hex = format!("{}{}", "19".repeat(16), "57".repeat(24));
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": [peer_hex],
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": payload_hex })),
            })
            .expect("ingest propagation");

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/unpeer"),
            Some(rmpv::Value::Binary(peer_bytes.to_vec())),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        );

        let Some(ControlResponse::Value(result)) = response else {
            panic!("expected peer unpeer result value");
        };
        assert_eq!(result["peer"].as_str(), Some(peer_hex.as_str()));
        assert_eq!(result["removed"].as_bool(), Some(true));
        assert_eq!(result["propagation_cleared"].as_u64(), Some(1));
        assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 2, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("peers result");
        assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn peer_unpeer_command_accepts_case_variant_existing_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        let peer_bytes = [0xB7; 16];
        let stored_peer = hex::encode(peer_bytes).to_ascii_uppercase();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": [stored_peer],
                })),
            })
            .expect("enable propagation");

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/unpeer"),
            Some(rmpv::Value::Binary(peer_bytes.to_vec())),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        );

        let Some(ControlResponse::Value(result)) = response else {
            panic!("expected peer unpeer result value");
        };
        assert_eq!(result["peer"].as_str(), Some(stored_peer.as_str()));
        assert_eq!(result["removed"].as_bool(), Some(true));
    }
}
