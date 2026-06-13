#[test]
fn invalid_stamp_propagation_remote_sync_preserves_peer_queue_with_retry_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation peer invalid stamp",
    }));
    daemon
        .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": "peer-invalid-stamp" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-invalid-stamp").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.45;
    }
    let pending = PropagationEntryRecord {
        transient_id: "b0".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_611,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-invalid-stamp", pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            97,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-invalid-stamp",
            }),
        ))
        .expect_err("invalid-stamp remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation peer invalid stamp");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 98, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-invalid-stamp"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.45));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-invalid-stamp")
            .expect("pending propagation"),
        vec![pending.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-invalid-stamp")
            .expect("handled ids")
            .is_empty(),
        "invalid-stamp response should not accept queued messages"
    );

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "invalid-stamp response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("invalid-stamp peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-invalid-stamp"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("invalid_stamp"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("invalid_stamp"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer invalid stamp")
    );
}

#[test]
fn retryable_propagation_remote_sync_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation peer invalid stamp",
    }));
    let peer = "peer-remote-retry-snapshot";
    daemon
        .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            97,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("retryable remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation peer invalid stamp");
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

#[test]
fn retryable_propagation_remote_sync_replays_restored_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation peer invalid stamp",
    }));
    let peer = "peer-remote-retry-restored-snapshot";
    daemon
        .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    let pending = PropagationEntryRecord {
        transient_id: "b5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_616,
        size_bytes: 24,
        stamp_value: None,
    };
    let handled = PropagationEntryRecord {
        transient_id: "b6".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store pending entry");
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
        record.restored_handled_ids.push(handled.transient_id.clone());
        record.restored_unhandled_ids.push(pending.transient_id.clone());
    }

    let err = daemon
        .handle_rpc(rpc_request(
            97,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("retryable remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation peer invalid stamp");

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
}

#[test]
fn unknown_numeric_propagation_remote_sync_preserves_peer_queue_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::InvalidData,
        message: "unexpected propagation control response",
    }));
    daemon
        .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": "peer-unknown-response" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unknown-response").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.35;
    }
    let pending = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_612,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unknown-response", pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            97,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-unknown-response",
            }),
        ))
        .expect_err("unknown numeric response should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "unexpected propagation control response");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 98, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-unknown-response"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.35));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-unknown-response")
            .expect("pending propagation"),
        vec![pending.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-unknown-response")
            .expect("handled ids")
            .is_empty(),
        "unknown numeric response should not accept queued messages"
    );

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "unknown numeric response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("unknown response peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-unknown-response"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("unexpected propagation control response")
    );
}

#[test]
fn failed_propagation_remote_sync_reports_effective_limits() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(FailingTransferLimitRemoteControlBridge {
        kind: std::io::ErrorKind::TimedOut,
        expected_sync_transfer_limit_kb: Some(42.5),
    }));
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-remote-sync-limit-fail" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-sync-limit-fail").expect("peer record");
        peer.propagation_transfer_limit = Some(100_000);
        peer.propagation_sync_limit = None;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-limit-fail",
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("failed remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-sync-limit-fail"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["propagation_transfer_limit"].as_u64(), Some(100_000));
    assert!(event.payload["propagation_sync_limit"].is_null());
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["propagation"]["sync_limit"].as_u64(), Some(42_500));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote sync failed")
    );
}
