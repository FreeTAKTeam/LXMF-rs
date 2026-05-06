use super::*;

pub(super) fn handle_peer_command(
    daemon: &RpcDaemon,
    path_hash: [u8; 16],
    data: Option<rmpv::Value>,
    error_invalid_data: u8,
    error_not_found: u8,
) -> Option<ControlResponse> {
    let method = if path_hash == control_path_hash("/pn/peer/sync") {
        "peer_sync"
    } else if path_hash == control_path_hash("/pn/peer/unpeer") {
        "peer_unpeer"
    } else {
        return None;
    };
    let Some(peer_hex) = peer_hex_from_data(data) else {
        return Some(ControlResponse::Code(error_invalid_data));
    };
    if !peer_exists(daemon, peer_hex.as_str()) {
        return Some(ControlResponse::Code(error_not_found));
    }
    let _ = daemon.handle_rpc(RpcRequest {
        id: 0,
        method: method.to_string(),
        params: Some(json!({ "peer": peer_hex })),
    });
    Some(ControlResponse::Bool(true))
}

fn peer_hex_from_data(data: Option<rmpv::Value>) -> Option<String> {
    match data {
        Some(rmpv::Value::Binary(bytes)) if bytes.len() == 16 => Some(hex::encode(bytes)),
        _ => None,
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
            rows.iter().any(|row| row.get("peer").and_then(Value::as_str) == Some(peer_hex))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR_INVALID_DATA: u8 = 0xF4;
    const ERROR_NOT_FOUND: u8 = 0xFD;

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
    fn peer_unpeer_command_delegates_to_daemon_rpc() {
        let daemon = RpcDaemon::test_instance();
        let peer_bytes = [0xB6; 16];
        let peer_hex = hex::encode(peer_bytes);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer_hex })),
            })
            .expect("seed peer");

        let response = handle_peer_command(
            &daemon,
            control_path_hash("/pn/peer/unpeer"),
            Some(rmpv::Value::Binary(peer_bytes.to_vec())),
            ERROR_INVALID_DATA,
            ERROR_NOT_FOUND,
        );

        assert!(matches!(response, Some(ControlResponse::Bool(true))));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 2, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("peers result");
        assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));
    }
}
