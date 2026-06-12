#[test]
fn propagation_remote_unpeer_reports_existing_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    let stored_peer = "Peer-Remote-Unpeer-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(82, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    let entry = PropagationEntryRecord {
        transient_id: "e3".repeat(32),
        destination: "1b".repeat(16),
        payload_hex: "1b".repeat(20),
        received_at: 1_700_000_806,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer, entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(
            83,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": request_peer,
            }),
        ))
        .expect("remote unpeer")
        .result
        .expect("remote unpeer result");
    assert_eq!(result["peer"].as_str(), Some(stored_peer));
    assert_eq!(result["result"]["peer"].as_str(), Some(stored_peer));
    assert_eq!(result["removed"].as_bool(), Some(true));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(1));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("stored peer unhandled")
            .is_empty()
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some(stored_peer));
    assert_eq!(event.payload["result"]["peer"].as_str(), Some(stored_peer));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
}

#[test]
fn propagation_remote_unpeer_trims_remote_before_bridge_event_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-remote-unpeer-trim" })))
        .expect("peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_unpeer",
            json!({
                "remote": "  remote-unpeer-trimmed  ",
                "peer": "peer-remote-unpeer-trim",
            }),
        ))
        .expect("remote unpeer with padded remote")
        .result
        .expect("remote unpeer result");

    assert_eq!(result["remote"].as_str(), Some("remote-unpeer-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-unpeer-trimmed"));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["remote"].as_str(), Some("remote-unpeer-trimmed"));
    assert_eq!(event.payload["result"]["remote"].as_str(), Some("remote-unpeer-trimmed"));
}

#[test]
fn propagation_remote_unpeer_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let unpeer_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::clone(&unpeer_calls),
    }));
    daemon
        .handle_rpc(rpc_request(83, "peer_sync", json!({ "peer": "peer-unpeer-blank-remote" })))
        .expect("peer sync");

    let rejected = daemon
        .handle_rpc(rpc_request(
            84,
            "propagation_remote_unpeer",
            json!({
                "remote": "   ",
                "peer": "peer-unpeer-blank-remote",
            }),
        ))
        .expect_err("blank remote-unpeer remote should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(unpeer_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 85, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .any(|row| row["peer"].as_str() == Some("peer-unpeer-blank-remote")),
        "blank remote-unpeer remote should preserve the local peer"
    );
}

#[test]
fn failed_propagation_remote_unpeer_preserves_local_peer_and_queue_state() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": "peer-remote-unpeer-fail" })))
        .expect("peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "1a".repeat(20),
        received_at: 1_700_000_802,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-remote-unpeer-fail", entry.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer-fail",
            }),
        ))
        .expect_err("remote unpeer failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(err.to_string(), "remote unpeer failed");

    let status = daemon
        .handle_rpc(rpc_request(82, "propagation_status", JsonValue::Null))
        .expect("propagation status after failed remote unpeer")
        .result
        .expect("status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("remote unpeer failed"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 81, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-remote-unpeer-fail"));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(20));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-remote-unpeer-fail")
            .expect("list unhandled"),
        vec![entry]
    );
}

#[test]
fn denied_access_propagation_remote_unpeer_breaks_peering_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteUnpeerErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node denied access",
    }));
    let peer = "peer-remote-unpeer-denied";
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync");
    let entry = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "24".repeat(16),
        payload_hex: "24".repeat(20),
        received_at: 1_700_000_812,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("remote unpeer access denial should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node denied access");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 81, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("list unhandled")
            .is_empty(),
        "access-denied remote unpeer should clear retryable local queue marks"
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("access_denied"));
    assert_eq!(event.payload["error"].as_str(), Some("propagation node denied access"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(1));
}

#[test]
fn failed_propagation_remote_unpeer_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-unpeer-fail-snapshot";
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "e3".repeat(32),
        destination: "1b".repeat(16),
        payload_hex: "1b".repeat(20),
        received_at: 1_700_000_803,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("remote unpeer failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let status = daemon
        .handle_rpc(rpc_request(81, "propagation_status", JsonValue::Null))
        .expect("propagation status after failed remote unpeer")
        .result
        .expect("status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("remote unpeer failed"));

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn failed_propagation_remote_unpeer_replays_restored_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-unpeer-fail-restored-snapshot";
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "e5".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_805,
        size_bytes: 20,
        stamp_value: None,
    };
    let handled_entry = PropagationEntryRecord {
        transient_id: "e6".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "1e".repeat(20),
        received_at: 1_700_000_806,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .upsert_propagation_entry(&handled_entry)
        .expect("store handled propagation entry");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
        record.restored_handled_ids.push(handled_entry.transient_id.clone());
        record.restored_unhandled_ids.push(entry.transient_id.clone());
    }

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("remote unpeer failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(handled_entry.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}
