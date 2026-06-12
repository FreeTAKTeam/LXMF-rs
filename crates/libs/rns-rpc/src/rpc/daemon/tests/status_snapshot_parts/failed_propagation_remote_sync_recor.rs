#[test]
fn failed_propagation_remote_sync_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-fail-snapshot";
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_615,
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
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
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
fn throttled_propagation_remote_sync_uses_python_retry_window_without_breaking_liveness() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::WouldBlock),
    }));
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-remote-throttled" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-throttled").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.75;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-throttled",
            }),
        ))
        .expect_err("remote sync throttling should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-throttled"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 180));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.75));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("throttled remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-throttled"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("throttled"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("throttled"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 180)
    );
}

#[test]
fn throttled_propagation_remote_sync_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::WouldBlock),
    }));
    let peer = "peer-remote-throttle-snapshot";
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_614,
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
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("remote sync throttling should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
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
fn throttled_remote_sync_matches_existing_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::WouldBlock),
    }));
    let stored_peer = "Peer-Remote-Throttled-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": stored_peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(stored_peer).expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.75;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": request_peer,
            }),
        ))
        .expect_err("remote sync throttling should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 80, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    let row = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some(stored_peer))
        .expect("stored peer row");
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 180));
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("throttled remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(stored_peer));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 180)
    );
}

#[test]
fn denied_access_propagation_remote_sync_breaks_peering_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node denied access",
    }));
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": "peer-remote-denied" })))
        .expect("initial peer sync");
    let entry = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "23".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_850,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-remote-denied", entry.transient_id.as_str())
        .expect("mark peer unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-denied",
            }),
        ))
        .expect_err("denied remote sync should still return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node denied access");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 80, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        !peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .any(|row| row["peer"].as_str() == Some("peer-remote-denied")),
        "denied access should break local peering"
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-remote-denied")
            .expect("list unhandled")
            .is_empty(),
        "denied access should clear peer propagation queue marks"
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("denied access unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-denied"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["reason"].as_str(), Some("access_denied"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
}

#[test]
fn identity_required_propagation_remote_sync_preserves_peer_with_retry_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node requires identity",
    }));
    daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-remote-needs-id" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-needs-id").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.8;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-needs-id",
            }),
        ))
        .expect_err("identity-required remote sync should still return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node requires identity");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 83, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-needs-id"))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.8));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "identity-required response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("identity-required peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-needs-id"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("no_identity"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("no_identity"));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation node requires identity")
    );
}
