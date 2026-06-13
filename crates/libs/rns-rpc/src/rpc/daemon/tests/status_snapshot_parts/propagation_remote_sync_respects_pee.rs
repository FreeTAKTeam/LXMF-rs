#[test]
fn propagation_remote_sync_respects_peer_backoff_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(89, "peer_sync", json!({ "peer": "peer-remote-backoff" })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-backoff").expect("peer record");
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = now_i64().saturating_add(12 * 60);
        peer.alive = false;
    }
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::clone(&sync_calls),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            90,
            "propagation_remote_sync",
            json!({
                "remote": "remote-backoff",
                "peer": "peer-remote-backoff",
            }),
        ))
        .expect("remote sync should postpone")
        .result
        .expect("remote sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(sync_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let status = daemon
        .handle_rpc(RpcRequest { id: 91, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0x00));
    assert_eq!(status["propagation"]["last_sync_started"], JsonValue::Null);
}

#[test]
fn propagation_remote_sync_backoff_does_not_require_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(92, "peer_sync", json!({ "peer": "peer-backoff-no-bridge" })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-backoff-no-bridge").expect("peer record");
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = now_i64().saturating_add(12 * 60);
        peer.alive = false;
    }

    let result = daemon
        .handle_rpc(rpc_request(
            93,
            "propagation_remote_sync",
            json!({
                "remote": "remote-backoff-no-bridge",
                "peer": "peer-backoff-no-bridge",
            }),
        ))
        .expect("remote sync should postpone before bridge lookup")
        .result
        .expect("remote sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 94, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0x00));
    assert_eq!(status["propagation"]["last_sync_started"], JsonValue::Null);
}

#[test]
fn propagation_remote_sync_backoff_records_preexisting_live_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-backoff-live-queue-snapshot";
    daemon
        .handle_rpc(rpc_request(92, "peer_sync", json!({ "peer": peer })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.sync_backoff = 12 * 60;
        record.next_sync_attempt = now_i64().saturating_add(12 * 60);
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "e6".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_617,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("seed live queue mark");

    let result = daemon
        .handle_rpc(rpc_request(
            93,
            "propagation_remote_sync",
            json!({
                "remote": "remote-backoff-no-bridge",
                "peer": peer,
            }),
        ))
        .expect("remote sync should postpone before bridge lookup")
        .result
        .expect("remote sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(pending.transient_id.as_str())]
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
fn propagation_remote_sync_missing_bridge_does_not_create_peer() {
    let daemon = RpcDaemon::test_instance();

    let err = daemon
        .handle_rpc(rpc_request(
            95,
            "propagation_remote_sync",
            json!({
                "remote": "remote-without-bridge",
                "peer": "peer-no-bridge",
            }),
        ))
        .expect_err("missing bridge should reject remote sync");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 96, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-no-bridge")),
        "missing remote bridge should not create local peer state"
    );

    let status = daemon
        .handle_rpc(RpcRequest { id: 97, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0x00));
    assert_eq!(status["propagation"]["last_sync_started"], JsonValue::Null);
}

#[test]
fn propagation_remote_sync_missing_bridge_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-sync-unavailable-snapshot";
    daemon
        .handle_rpc(rpc_request(95, "peer_sync", json!({ "peer": peer })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let pending = PropagationEntryRecord {
        transient_id: "e5".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_805,
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
            96,
            "propagation_remote_sync",
            json!({
                "remote": "remote-without-bridge",
                "peer": peer,
            }),
        ))
        .expect_err("missing bridge should reject remote sync");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

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
fn propagation_remote_sync_missing_bridge_replays_restored_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-sync-unavailable-restored-snapshot";
    daemon
        .handle_rpc(rpc_request(95, "peer_sync", json!({ "peer": peer })))
        .expect("seed peer");

    let pending = PropagationEntryRecord {
        transient_id: "e7".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(20),
        received_at: 1_700_000_807,
        size_bytes: 20,
        stamp_value: None,
    };
    let handled = PropagationEntryRecord {
        transient_id: "e8".repeat(32),
        destination: "20".repeat(16),
        payload_hex: "20".repeat(20),
        received_at: 1_700_000_808,
        size_bytes: 20,
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
            96,
            "propagation_remote_sync",
            json!({
                "remote": "remote-without-bridge",
                "peer": peer,
            }),
        ))
        .expect_err("missing bridge should reject remote sync");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

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
fn propagation_remote_sync_missing_bridge_reports_existing_peer_failure_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-sync-unavailable-event";
    daemon
        .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": peer })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let pending = PropagationEntryRecord {
        transient_id: "e6".repeat(32),
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
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            97,
            "propagation_remote_sync",
            json!({
                "remote": "remote-without-bridge",
                "peer": peer,
            }),
        ))
        .expect_err("missing bridge should reject remote sync");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let status = daemon
        .handle_rpc(RpcRequest { id: 98, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(status["propagation"]["state_name"].as_str(), Some("failed"));
    assert_eq!(
        status["propagation"]["last_sync_error"].as_str(),
        Some("remote control bridge unavailable")
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 99, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("peer should remain queued for retry");
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("missing bridge peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-without-bridge"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote control bridge unavailable")
    );
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
}

#[test]
fn propagation_remote_sync_missing_bridge_records_case_insensitive_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Remote-Sync-Unavailable-Snapshot-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": stored_peer })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let pending = PropagationEntryRecord {
        transient_id: "ea".repeat(32),
        destination: "20".repeat(16),
        payload_hex: "20".repeat(20),
        received_at: 1_700_000_808,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer, pending.transient_id.as_str())
        .expect("seed live queue mark");

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-without-bridge",
                "peer": request_peer,
            }),
        ))
        .expect_err("missing bridge should reject remote sync");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
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
