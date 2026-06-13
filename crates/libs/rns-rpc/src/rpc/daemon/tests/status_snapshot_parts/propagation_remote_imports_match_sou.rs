#[test]
fn propagation_remote_imports_match_source_peer_case_insensitively_like_python() {
    let sync_payload = b"remote-sync-case-source-payload";
    let sync_payload_hex = hex::encode(sync_payload);
    let sync_transient_id = hex::encode(Sha256::digest(sync_payload));
    let sync_source_peer = "Remote-Sync-Case-Source";
    let sync_relay_peer = "remote-sync-case-relay";
    let sync_daemon = RpcDaemon::test_instance();
    sync_daemon
        .handle_rpc(rpc_request(76, "peer_sync", json!({ "peer": sync_source_peer })))
        .expect("seed sync source peer");
    sync_daemon
        .handle_rpc(rpc_request(77, "peer_sync", json!({ "peer": sync_relay_peer })))
        .expect("seed sync relay peer");
    sync_daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [{
                "transient_id": sync_transient_id,
                "payload_hex": sync_payload_hex,
            }],
        })),
    }));

    sync_daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "remote-sync-case-source",
            }),
        ))
        .expect("remote sync from source peer");
    assert!(
        sync_daemon
            .store
            .list_peer_unhandled_propagation(sync_source_peer)
            .expect("sync source unhandled")
            .is_empty(),
        "remote sync source should not be offered the payload it supplied"
    );
    assert_eq!(
        sync_daemon
            .store
            .list_peer_handled_propagation_ids(sync_source_peer)
            .expect("sync source handled ids"),
        vec![sync_transient_id.clone()]
    );
    let sync_relay_pending = sync_daemon
        .store
        .list_peer_unhandled_propagation(sync_relay_peer)
        .expect("sync relay pending");
    assert_eq!(sync_relay_pending.len(), 1);
    assert_eq!(sync_relay_pending[0].transient_id, sync_transient_id);

    let fetch_payload = b"remote-fetch-case-source-payload";
    let fetch_payload_hex = hex::encode(fetch_payload);
    let fetch_transient_id = hex::encode(Sha256::digest(fetch_payload));
    let fetch_source_peer = "Remote-Fetch-Case-Source";
    let fetch_relay_peer = "remote-fetch-case-relay";
    let fetch_daemon = RpcDaemon::test_instance();
    fetch_daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": fetch_source_peer })))
        .expect("seed fetch source peer");
    fetch_daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": fetch_relay_peer })))
        .expect("seed fetch relay peer");
    fetch_daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": fetch_transient_id,
                "payload_hex": fetch_payload_hex,
            }],
        })),
    }));

    fetch_daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_remote_fetch",
            json!({ "remote": "remote-fetch-case-source" }),
        ))
        .expect("remote fetch from source peer");
    assert!(
        fetch_daemon
            .store
            .list_peer_unhandled_propagation(fetch_source_peer)
            .expect("fetch source unhandled")
            .is_empty(),
        "remote fetch source should not be offered the payload it supplied"
    );
    assert_eq!(
        fetch_daemon
            .store
            .list_peer_handled_propagation_ids(fetch_source_peer)
            .expect("fetch source handled ids"),
        vec![fetch_transient_id.clone()]
    );
    let fetch_relay_pending = fetch_daemon
        .store
        .list_peer_unhandled_propagation(fetch_relay_peer)
        .expect("fetch relay pending");
    assert_eq!(fetch_relay_pending.len(), 1);
    assert_eq!(fetch_relay_pending[0].transient_id, fetch_transient_id);
}

#[test]
fn propagation_remote_fetch_trims_remote_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 0,
            "fetched_count": 0,
            "messages": [],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_fetch",
            json!({
                "remote": "  remote-fetch-trimmed  ",
            }),
        ))
        .expect("remote fetch with padded remote")
        .result
        .expect("remote fetch result");

    assert_eq!(result["remote"].as_str(), Some("remote-fetch-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-fetch-trimmed"));
}

#[test]
fn propagation_remote_fetch_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let fetch_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::clone(&fetch_calls),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_fetch",
            json!({
                "remote": "   ",
            }),
        ))
        .expect_err("blank remote fetch node should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(fetch_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn duplicate_propagation_remote_fetch_queues_known_payload_without_double_counting() {
    let payload = b"duplicate-remote-fetch-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-fetch-known" })))
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
            73,
            "propagation_remote_fetch",
            json!({ "remote": "remote-node" }),
        ))
        .expect("initial remote fetch");
    daemon
        .store
        .clear_peer_propagation_marks("peer-fetch-known")
        .expect("clear peer marks");
    let second = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({ "remote": "remote-node" }),
        ))
        .expect("duplicate remote fetch")
        .result
        .expect("duplicate remote fetch result");
    assert_eq!(second["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(second["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["result"]["imported_ids"], json!([]));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-fetch-known")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

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
fn duplicate_propagation_remote_fetch_does_not_double_count_source_receive_bytes() {
    let payload = b"duplicate-remote-fetch-source-accounting-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-fetch-duplicate-source";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
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

    for request_id in [73, 74] {
        daemon
            .handle_rpc(rpc_request(
                request_id,
                "propagation_remote_fetch",
                json!({ "remote": source_peer }),
            ))
            .expect("remote fetch from source peer");
    }

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
    assert_eq!(source_row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids"),
        vec![transient_id]
    );
}

#[test]
fn propagation_remote_fetch_deduplicates_same_response_for_peer_incoming_like_python() {
    let payload = b"duplicate-same-fetch-response-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-fetch-dedup-source";
    let relay_peer = "remote-fetch-dedup-relay";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(73, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 2,
            "fetched_count": 2,
            "messages": [
                {
                    "transient_id": transient_id,
                    "payload_hex": payload_hex,
                },
                {
                    "transient_id": transient_id,
                    "payload_hex": payload_hex,
                },
            ],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({ "remote": source_peer }),
        ))
        .expect("remote fetch")
        .result
        .expect("remote fetch result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(result["result"]["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids"),
        vec![transient_id.clone()]
    );
    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer)
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

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
    assert_eq!(source_row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
}

#[test]
fn propagation_remote_fetch_preserves_transfer_limited_peer_queue_mark_like_python() {
    let payload = b"remote-fetch-retry-transfer-limited-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-fetch-retry-limit" })))
        .expect("seed relay peer");
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: "23".repeat(16),
            payload_hex: payload_hex.clone(),
            received_at: 1_700_000_701,
            size_bytes: payload.len() as u64,
            stamp_value: None,
        })
        .expect("seed known propagation entry");
    daemon
        .store
        .mark_peer_transfer_limited_propagation("peer-fetch-retry-limit", transient_id.as_str())
        .expect("mark transfer limited");
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

    let result = daemon
        .handle_rpc(rpc_request(73, "propagation_remote_fetch", json!({ "remote": "remote-node" })))
        .expect("remote fetch")
        .result
        .expect("remote fetch result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(result["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([]));

    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-fetch-retry-limit")
        .expect("pending relay entries");
    assert!(pending.is_empty());
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-fetch-retry-limit")
            .expect("handled relay ids"),
        vec![transient_id]
    );
}
