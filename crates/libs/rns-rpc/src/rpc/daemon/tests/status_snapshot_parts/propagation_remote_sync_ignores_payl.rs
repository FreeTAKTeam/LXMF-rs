#[test]
fn propagation_remote_sync_ignores_payload_byte_count_rows_during_import() {
    let payload = b"remote-sync-after-payload-byte-count-row";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": {
                "offered": 1,
                "outgoing": 1,
                "incoming": 0,
                "unhandled": 0,
                "handled_ids": [transient_id],
                "unhandled_ids": [],
            },
            "propagation": {
                "synced": true,
                "transferred": 1,
                "messages": [
                    {
                        "transient_id": "11".repeat(32),
                        "payload_bytes": payload.len(),
                    },
                    {
                        "transient_id": transient_id,
                        "payload": payload.to_vec(),
                    }
                ],
            },
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-byte-count-sync",
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(result["result"]["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after payload-byte count row remote sync")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn duplicate_propagation_remote_sync_import_does_not_double_count_received() {
    let payload = b"duplicate-remote-sync-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "imported_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let mut second = JsonValue::Null;
    for request_id in [73, 74] {
        let result = daemon
            .handle_rpc(rpc_request(
                request_id,
                "propagation_remote_sync",
                json!({
                    "remote": "remote-node",
                    "peer": "peer-a",
                }),
            ))
            .expect("remote sync")
            .result
            .expect("remote sync result");
        second = result;
    }
    assert_eq!(second["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(second["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["result"]["imported_ids"], json!([]));
    assert_eq!(
        second["peer_sync"]["propagation"]["duplicate_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        second["peer_sync"]["messages"]["handled_ids"]
            .as_array()
            .expect("source handled ids"),
        &[json!(transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-a")
            .expect("source handled ids"),
        vec![transient_id]
    );

    let status = daemon
        .handle_rpc(RpcRequest { id: 75, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(1)
    );
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn propagation_remote_fetch_imports_payloads_into_local_store() {
    let payload = b"remote-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-fetch-relay" })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "imported_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "destination": "23".repeat(16),
                "payload_hex": payload_hex,
                "received_at": 1_700_000_700i64,
                "stamp_value": 6,
            }],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect("remote fetch")
        .result
        .expect("remote fetch result");
    assert_eq!(result["propagation"]["sync_state"].as_u64(), Some(0x07));
    assert_eq!(result["propagation"]["state_name"].as_str(), Some("completed"));
    assert_eq!(result["propagation"]["sync_progress"].as_f64(), Some(1.0));
    assert!(result["propagation"]["last_sync_started"].as_i64().is_some());
    assert!(result["propagation"]["last_sync_completed"].as_i64().is_some());
    assert_eq!(result["propagation"]["last_sync_error"], JsonValue::Null);
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(result["result"]["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after remote import")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-fetch-relay")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);
}

#[test]
fn propagation_remote_fetch_marks_source_received_and_queues_other_peers() {
    let payload = b"remote-fetch-source-peer-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-fetch-source";
    let relay_peer = "peer-fetch-source-relay";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(73, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({ "remote": source_peer }),
        ))
        .expect("remote fetch from source peer");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(source_peer)
            .expect("source unhandled")
            .is_empty(),
        "remote source should not be offered the payload it supplied"
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids"),
        vec![transient_id.clone()]
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 75, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer))
        .expect("source peer row");
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(source_row["alive"].as_bool(), Some(true));
    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer)
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);
}

#[test]
fn propagation_remote_fetch_success_clears_source_peer_retry_backoff() {
    let payload = b"remote-fetch-source-peer-recovery-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-fetch-source-recovery";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(source_peer).expect("source peer record");
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = now_i64().saturating_add(12 * 60);
    }
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_fetch",
            json!({ "remote": source_peer }),
        ))
        .expect("remote fetch from recovered source");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 74, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer))
        .expect("source peer row");
    assert_eq!(source_row["alive"].as_bool(), Some(true));
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(source_row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(source_row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        source_row["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(transient_id.as_str())]
    );
}

#[test]
fn propagation_remote_fetch_marks_inactive_source_received_for_later_activation_like_python() {
    let payload = b"remote-fetch-inactive-source-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-fetch-late-source";
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({ "remote": source_peer }),
        ))
        .expect("remote fetch from inactive source");

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("inactive source handled ids"),
        vec![transient_id.clone()],
        "inactive source should be marked received before later peer activation"
    );

    let sync = daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": source_peer })))
        .expect("activate source peer")
        .result
        .expect("peer sync result");
    assert_eq!(sync["propagation"]["transferred"].as_u64(), Some(0));
    assert!(
        sync["propagation"]["messages"].as_array().expect("transferred messages").is_empty()
    );
    assert_eq!(sync["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(
        sync["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(transient_id.as_str())]
    );
}
