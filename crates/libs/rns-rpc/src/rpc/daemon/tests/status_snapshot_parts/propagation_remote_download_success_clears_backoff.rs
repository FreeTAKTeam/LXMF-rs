#[test]
fn propagation_remote_download_success_clears_source_peer_retry_backoff() {
    let payload = b"remote-download-source-peer-recovery-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-download-source-recovery";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": source_peer })))
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
            "downloaded_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_download",
            json!({ "remote": source_peer }),
        ))
        .expect("remote download from recovered source");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 80, method: "list_peers".to_string(), params: None })
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
fn propagation_remote_download_trims_remote_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 0,
            "messages": [],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_download",
            json!({
                "remote": "  remote-download-trimmed  ",
            }),
        ))
        .expect("remote download with padded remote")
        .result
        .expect("remote download result");

    assert_eq!(result["remote"].as_str(), Some("remote-download-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-download-trimmed"));
}

#[test]
fn propagation_remote_download_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let download_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::clone(&download_calls),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_download",
            json!({
                "remote": "   ",
            }),
        ))
        .expect_err("blank remote download node should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(download_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn duplicate_propagation_remote_download_queues_known_payload_without_double_counting() {
    let payload = b"duplicate-remote-download-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-download-known" })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_download",
            json!({ "remote": "remote-node" }),
        ))
        .expect("initial remote download");
    daemon
        .store
        .clear_peer_propagation_marks("peer-download-known")
        .expect("clear peer marks");
    let second = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_download",
            json!({ "remote": "remote-node" }),
        ))
        .expect("duplicate remote download")
        .result
        .expect("duplicate remote download result");
    assert_eq!(second["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(second["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["result"]["imported_ids"], json!([]));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-download-known")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

    let status = daemon
        .handle_rpc(RpcRequest { id: 78, method: "propagation_status".to_string(), params: None })
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
fn propagation_remote_download_forwards_transfer_limit_to_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TransferLimitRemoteControlBridge));

    daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_download",
            json!({
                "remote": "remote-node",
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect("remote download with transfer limit");
}

#[test]
fn propagation_remote_fetch_missing_bridge_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-fetch-unavailable-snapshot";
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": peer })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let pending = PropagationEntryRecord {
        transient_id: "e9".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(20),
        received_at: 1_700_000_807,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("seed live queue mark");

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-without-bridge",
            }),
        ))
        .expect_err("missing bridge should reject remote fetch");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let status = daemon
        .handle_rpc(rpc_request(80, "propagation_status", JsonValue::Null))
        .expect("propagation status after missing fetch bridge")
        .result
        .expect("status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(
        propagation["last_sync_error"].as_str(),
        Some("remote control bridge unavailable")
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
}

#[test]
fn propagation_remote_download_missing_bridge_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-download-unavailable-snapshot";
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": peer })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let pending = PropagationEntryRecord {
        transient_id: "e8".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "1e".repeat(20),
        received_at: 1_700_000_806,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("seed live queue mark");

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_download",
            json!({
                "remote": "remote-without-bridge",
            }),
        ))
        .expect_err("missing bridge should reject remote download");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let status = daemon
        .handle_rpc(rpc_request(80, "propagation_status", JsonValue::Null))
        .expect("propagation status after missing download bridge")
        .result
        .expect("status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(
        propagation["last_sync_error"].as_str(),
        Some("remote control bridge unavailable")
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
}

#[test]
fn propagation_remote_fetch_success_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 0,
            "fetched_count": 0,
            "messages": [],
        })),
    }));
    let peer = "peer-remote-fetch-success-snapshot";
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "e7".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(24),
        received_at: 1_700_000_805,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");

    daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect("remote fetch success should preserve queued retry snapshot");
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("pending propagation"),
        vec![pending.clone()]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
}
