#[test]
fn invalid_peering_key_propagation_remote_sync_preserves_peer_with_retry_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation peer invalid peering key",
    }));
    daemon
        .handle_rpc(rpc_request(84, "peer_sync", json!({ "peer": "peer-invalid-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-invalid-key").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.7;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            85,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-invalid-key",
            }),
        ))
        .expect_err("invalid peering-key remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation peer invalid peering key");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 86, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-invalid-key"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.7));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "invalid peering-key response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("invalid peering-key peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-invalid-key"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("invalid_key"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("invalid_key"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer invalid peering key")
    );
}

#[test]
fn invalid_data_propagation_remote_sync_preserves_peer_with_retry_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::InvalidInput,
        message: "propagation node rejected the request",
    }));
    daemon
        .handle_rpc(rpc_request(87, "peer_sync", json!({ "peer": "peer-invalid-data" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-invalid-data").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.6;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            88,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-invalid-data",
            }),
        ))
        .expect_err("invalid-data remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), "propagation node rejected the request");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 89, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-invalid-data"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.6));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "invalid-data response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("invalid-data peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-invalid-data"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("invalid_data"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("invalid_data"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation node rejected the request")
    );
}

#[test]
fn timeout_propagation_remote_sync_preserves_peer_with_retry_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::TimedOut,
        message: "propagation peer timed out",
    }));
    daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-timeout" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-timeout").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }
    let pending = PropagationEntryRecord {
        transient_id: "fa".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(24),
        received_at: 1_700_001_010,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-timeout", pending.transient_id.as_str())
        .expect("mark timeout peer unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            91,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-timeout",
            }),
        ))
        .expect_err("timeout remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(err.to_string(), "propagation peer timed out");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 92, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-timeout"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["state"].as_u64(), Some(0));
    assert_eq!(row["state_name"].as_str(), Some("idle"));
    assert_eq!(row["sync_schedule_state"].as_str(), Some("backoff"));
    assert_eq!(row["sync_schedule_reason"].as_str(), Some("backoff"));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert_eq!(row["messages"]["unhandled_ids"], json!([pending.transient_id.as_str()]));

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("peer-timeout").expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
    drop(peers);

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "timeout response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("timeout peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-timeout"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["state"].as_u64(), Some(0xfe));
    assert_eq!(event.payload["state_name"].as_str(), Some("failed"));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("timeout"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("timeout"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer timed out")
    );
    assert_eq!(event.payload["propagation"]["state_name"].as_str(), Some("failed"));
}

#[test]
fn not_found_propagation_remote_sync_preserves_peer_with_retry_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::NotFound,
        message: "propagation peer not found",
    }));
    daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": "peer-not-found" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-not-found").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.4;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            94,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-not-found",
            }),
        ))
        .expect_err("not-found remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(err.to_string(), "propagation peer not found");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 95, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-not-found"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.4));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "not-found response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("not-found peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-not-found"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("not_found"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("not_found"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer not found")
    );
}
