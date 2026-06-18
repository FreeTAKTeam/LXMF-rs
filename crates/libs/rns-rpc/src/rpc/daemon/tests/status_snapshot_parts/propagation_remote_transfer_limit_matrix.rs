struct ExpectedRemoteTransferLimitBridge {
    expected_sync_transfer_limit_kb: Option<f64>,
    expected_fetch_transfer_limit_kb: Option<f64>,
    expected_download_transfer_limit_kb: Option<f64>,
    result: JsonValue,
}

impl RemoteControlBridge for ExpectedRemoteTransferLimitBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "remote": remote, "status": "ok" }))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_sync_transfer_limit_kb);
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        result["peer"] = json!(peer);
        Ok(result)
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_download_transfer_limit_kb);
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        Ok(result)
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_fetch_transfer_limit_kb);
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        Ok(result)
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "remote": remote, "peer": peer, "unpeered": true }))
    }
}

#[test]
fn propagation_remote_fetch_forwards_request_transfer_limit_to_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(ExpectedRemoteTransferLimitBridge {
        expected_sync_transfer_limit_kb: None,
        expected_fetch_transfer_limit_kb: Some(42.5),
        expected_download_transfer_limit_kb: None,
        result: json!({ "available_count": 0, "fetched_count": 0, "messages": [] }),
    }));

    daemon
        .handle_rpc(rpc_request(
            86,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect("remote fetch with transfer limit");
}

fn remote_transfer_limit_bridge_result(method: &str, messages: Vec<JsonValue>) -> JsonValue {
    match method {
        "propagation_remote_sync" => json!({ "synced": true, "messages": messages }),
        "propagation_remote_fetch" => {
            json!({ "available_count": messages.len(), "fetched_count": messages.len(), "messages": messages })
        }
        "propagation_remote_download" => {
            json!({ "downloaded_count": messages.len(), "messages": messages })
        }
        _ => unreachable!("remote transfer-limit method: {method}"),
    }
}

fn invoke_remote_transfer_limit_import(
    daemon: &RpcDaemon,
    method: &str,
    remote: &str,
    source_peer: &str,
    messages: Vec<JsonValue>,
) {
    daemon.set_remote_control_bridge(Arc::new(ExpectedRemoteTransferLimitBridge {
        expected_sync_transfer_limit_kb: None,
        expected_fetch_transfer_limit_kb: None,
        expected_download_transfer_limit_kb: None,
        result: remote_transfer_limit_bridge_result(method, messages),
    }));
    let params = if method == "propagation_remote_sync" {
        json!({ "remote": remote, "peer": source_peer })
    } else {
        json!({ "remote": source_peer })
    };
    daemon
        .handle_rpc(rpc_request(90, method, params))
        .expect("remote import for transfer-limit matrix");
}

#[test]
fn propagation_remote_download_transfer_limit_matrix_completes_individually_oversized_imports() {
    for (method, byte) in [
        ("propagation_remote_sync", 0x41_u8),
        ("propagation_remote_fetch", 0x42_u8),
        ("propagation_remote_download", 0x43_u8),
    ] {
        let payload = vec![byte; 100];
        let payload_hex = hex::encode(payload.as_slice());
        let transient_id = hex::encode(Sha256::digest(payload.as_slice()));
        let source_peer = format!("remote-transfer-limit-oversized-source-{byte}");
        let relay_peer = format!("remote-transfer-limit-oversized-relay-{byte}");
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(rpc_request(91, "peer_sync", json!({ "peer": source_peer })))
            .expect("seed source peer");
        daemon
            .handle_rpc(rpc_request(92, "peer_sync", json!({ "peer": relay_peer })))
            .expect("seed relay peer");
        {
            let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
            let relay = peers.get_mut(relay_peer.as_str()).expect("relay peer");
            relay.propagation_transfer_limit = Some(80);
            relay.propagation_sync_limit = Some(1_000);
            relay.propagation_stamp_cost = Some(0);
        }
        invoke_remote_transfer_limit_import(
            &daemon,
            method,
            "remote-transfer-limit-oversized-node",
            source_peer.as_str(),
            vec![json!({ "transient_id": transient_id, "payload_hex": payload_hex })],
        );

        let limited = daemon
            .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": relay_peer })))
            .expect("relay peer sync transfer-limits oversized payload")
            .result
            .expect("relay peer sync result");
        assert_eq!(limited["synced"].as_bool(), Some(true), "{method}");
        assert_eq!(limited["propagation"]["transfer_limited"].as_u64(), Some(1), "{method}");
        assert_eq!(limited["propagation"]["skipped"].as_u64(), Some(0), "{method}");
        assert_eq!(
            limited["propagation"]["transfer_limited_ids"]
                .as_array()
                .expect("transfer-limited ids"),
            &[json!(transient_id.as_str())],
            "{method}"
        );
        assert_eq!(
            daemon
                .store
                .list_peer_handled_propagation_ids(relay_peer.as_str())
                .expect("handled ids"),
            vec![transient_id.clone()],
            "{method}"
        );
        assert!(
            daemon
                .store
                .list_peer_unhandled_propagation(relay_peer.as_str())
                .expect("pending relay entries")
                .is_empty(),
            "{method} oversized entry should not remain retryable"
        );

        {
            let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
            let relay = peers.get_mut(relay_peer.as_str()).expect("relay peer");
            relay.propagation_transfer_limit = Some(1_000);
            relay.propagation_sync_limit = Some(1_000);
        }
        let retry = daemon
            .handle_rpc(rpc_request(
                94,
                "peer_sync",
                json!({
                    "peer": relay_peer,
                    "transfer_limit_kb": 1.0,
                }),
            ))
            .expect("relay peer sync after larger limit")
            .result
            .expect("retry result");
        assert_eq!(retry["propagation"]["transferred"].as_u64(), Some(0), "{method}");
        assert_eq!(retry["propagation"]["transfer_limited"].as_u64(), Some(0), "{method}");
        assert!(
            retry["propagation"]["transferred_ids"]
                .as_array()
                .expect("transferred ids")
                .is_empty(),
            "{method}"
        );
    }
}

#[test]
fn propagation_remote_download_transfer_limit_matrix_keeps_cumulative_budget_skips_retryable() {
    for (method, byte) in [
        ("propagation_remote_sync", 0x51_u8),
        ("propagation_remote_fetch", 0x52_u8),
        ("propagation_remote_download", 0x53_u8),
    ] {
        let first_payload = vec![byte; 10];
        let second_payload = vec![byte.saturating_add(1); 40];
        let first_hex = hex::encode(first_payload.as_slice());
        let second_hex = hex::encode(second_payload.as_slice());
        let first_id = hex::encode(Sha256::digest(first_payload.as_slice()));
        let second_id = hex::encode(Sha256::digest(second_payload.as_slice()));
        let source_peer = format!("remote-transfer-limit-budget-source-{byte}");
        let relay_peer = format!("remote-transfer-limit-budget-relay-{byte}");
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(rpc_request(95, "peer_sync", json!({ "peer": source_peer })))
            .expect("seed source peer");
        daemon
            .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": relay_peer })))
            .expect("seed relay peer");
        {
            let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
            let relay = peers.get_mut(relay_peer.as_str()).expect("relay peer");
            relay.propagation_transfer_limit = Some(1_000);
            relay.propagation_sync_limit = Some(80);
            relay.propagation_stamp_cost = Some(0);
            relay.sync_strategy = 0;
        }
        invoke_remote_transfer_limit_import(
            &daemon,
            method,
            "remote-transfer-limit-budget-node",
            source_peer.as_str(),
            vec![
                json!({ "transient_id": first_id, "payload_hex": first_hex }),
                json!({ "transient_id": second_id, "payload_hex": second_hex }),
            ],
        );

        let limited = daemon
            .handle_rpc(rpc_request(
                97,
                "peer_sync",
                json!({
                    "peer": relay_peer,
                    "transfer_limit_kb": 1.0,
                }),
            ))
            .expect("relay peer sync with cumulative budget")
            .result
            .expect("limited result");
        assert_eq!(limited["propagation"]["transferred"].as_u64(), Some(1), "{method}");
        assert_eq!(limited["propagation"]["skipped"].as_u64(), Some(1), "{method}");
        assert_eq!(limited["propagation"]["transfer_limited"].as_u64(), Some(0), "{method}");
        assert_eq!(
            limited["propagation"]["transferred_ids"]
                .as_array()
                .expect("transferred ids"),
            &[json!(first_id.as_str())],
            "{method}"
        );
        assert_eq!(
            limited["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
            &[json!(second_id.as_str())],
            "{method}"
        );
        assert_eq!(
            daemon
                .store
                .list_peer_unhandled_propagation(relay_peer.as_str())
                .expect("pending relay entries")
                .into_iter()
                .map(|entry| entry.transient_id)
                .collect::<Vec<_>>(),
            vec![second_id.clone()],
            "{method}"
        );

        {
            let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
            let relay = peers.get_mut(relay_peer.as_str()).expect("relay peer");
            relay.propagation_sync_limit = Some(1_000);
        }
        let retry = daemon
            .handle_rpc(rpc_request(
                98,
                "peer_sync",
                json!({
                    "peer": relay_peer,
                    "transfer_limit_kb": 1.0,
                }),
            ))
            .expect("relay peer sync retries skipped payload")
            .result
            .expect("retry result");
        assert_eq!(retry["propagation"]["transferred"].as_u64(), Some(1), "{method}");
        assert_eq!(retry["propagation"]["skipped"].as_u64(), Some(0), "{method}");
        assert_eq!(
            retry["propagation"]["transferred_ids"]
                .as_array()
                .expect("retry transferred ids"),
            &[json!(second_id.as_str())],
            "{method}"
        );
        assert!(
            daemon
                .store
                .list_peer_unhandled_propagation(relay_peer.as_str())
                .expect("pending relay entries")
                .is_empty(),
            "{method} cumulative skip should clear after later successful request"
        );
    }
}
