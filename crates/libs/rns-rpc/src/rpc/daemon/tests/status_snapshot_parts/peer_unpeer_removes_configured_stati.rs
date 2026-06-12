#[test]
fn peer_unpeer_removes_configured_static_peer_membership_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-unpeer"],
            }),
        ))
        .expect("enable static peer");
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": "peer-static-unpeer" })))
        .expect("sync static peer");

    let unpeered = daemon
        .handle_rpc(rpc_request(80, "peer_unpeer", json!({ "peer": "peer-static-unpeer" })))
        .expect("unpeer static peer");
    assert!(unpeered.error.is_none());

    let status = daemon
        .handle_rpc(RpcRequest { id: 81, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["static_peers"].as_array().expect("static peers"),
        &[] as &[JsonValue]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 82, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"].as_array().expect("peer rows").is_empty(),
        "explicit unpeer should not be undone by static-peer activation"
    );
}

#[test]
fn unpeered_peers_do_not_consume_max_peer_capacity() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_enable",
            json!({
                "enabled": true,
                "max_peers": 1,
            }),
        ))
        .expect("enable propagation");

    let first = daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-a" })))
        .expect("sync peer-a");
    assert!(first.error.is_none());

    let blocked = daemon.handle_rpc(rpc_request(82, "peer_sync", json!({ "peer": "peer-b" })));
    assert!(blocked.is_err(), "second peer should be rejected while capacity is full");

    let unpeered = daemon
        .handle_rpc(rpc_request(83, "peer_unpeer", json!({ "peer": "peer-a" })))
        .expect("unpeer peer-a");
    assert!(unpeered.error.is_none());

    let replacement = daemon
        .handle_rpc(rpc_request(84, "peer_sync", json!({ "peer": "peer-b" })))
        .expect("sync replacement peer-b");
    assert!(replacement.error.is_none(), "replacement peer should be admitted after unpeer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 86, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["peer"].as_str(), Some("peer-b"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 85, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));
}

#[test]
fn peer_unpeer_snapshot_count_ignores_unpeered_records() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(87, "peer_sync", json!({ "peer": "peer-active" })))
        .expect("sync active peer");
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        guard.insert(
            "peer-unpeered".to_string(),
            daemon.transient_peer_record(
                "peer-unpeered".to_string(),
                1_700_000_900,
                Vec::new(),
                None,
                None,
                Some("unpeered".to_string()),
            ),
        );
    }

    daemon
        .handle_rpc(rpc_request(88, "peer_unpeer", json!({ "peer": "peer-active" })))
        .expect("unpeer active peer");

    let status = daemon
        .handle_rpc(RpcRequest { id: 89, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(0));
}

#[test]
fn peer_sync_reactivates_persisted_unpeered_record() {
    let daemon = RpcDaemon::test_instance();
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        guard.insert(
            "peer-rejoin".to_string(),
            daemon.transient_peer_record(
                "peer-rejoin".to_string(),
                i64::MAX,
                Vec::new(),
                None,
                None,
                Some("unpeered".to_string()),
            ),
        );
    }

    let result = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-rejoin" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("manual"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 91, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 92, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-rejoin"));
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
}

#[test]
fn peer_sync_does_not_reactivate_unpeered_non_static_when_static_only() {
    let daemon = RpcDaemon::test_instance();
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        guard.insert(
            "peer-static-only-rejoin".to_string(),
            daemon.transient_peer_record(
                "peer-static-only-rejoin".to_string(),
                1_700_000_901,
                Vec::new(),
                None,
                None,
                Some("unpeered".to_string()),
            ),
        );
    }
    daemon
        .handle_rpc(rpc_request(
            89,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-allowed"],
            }),
        ))
        .expect("enable static-only propagation");

    let blocked = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-static-only-rejoin" })))
        .expect_err("static-only policy should reject unpeered non-static reactivation");
    assert_eq!(blocked.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        blocked.to_string().contains("from_static_only"),
        "unexpected rejection error: {blocked}"
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 91, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    let rejoin = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-only-rejoin"))
        .expect("persisted unpeered row");
    assert_eq!(rejoin["peer_type"].as_str(), Some("unpeered"));
    assert!(rows.iter().any(|row| {
        row["peer"].as_str() == Some("peer-static-allowed")
            && row["peer_type"].as_str() == Some("static")
    }));
    let status = daemon
        .handle_rpc(RpcRequest { id: 92, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));
}

#[test]
fn peer_sync_reactivation_clears_unpeered_queue_snapshot() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-rejoin-clears-queue";
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        let mut record = daemon.transient_peer_record(
            peer.to_string(),
            1_700_000_902,
            Vec::new(),
            None,
            None,
            Some("unpeered".to_string()),
        );
        record.restored_handled_ids.push("aa".repeat(32));
        record.restored_unhandled_ids.push("bb".repeat(32));
        record.last_sync_attempt = now_i64();
        record.next_sync_attempt = now_i64().saturating_add(12 * 60);
        record.sync_backoff = 12 * 60;
        guard.insert(peer.to_string(), record);
    }

    let result = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("manual"));
    assert!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids").is_empty()
    );
    assert!(
        result["messages"]["unhandled_ids"]
            .as_array()
            .expect("result unhandled ids")
            .is_empty()
    );
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    assert_eq!(record.sync_backoff, 0);
    assert_eq!(record.next_sync_attempt, 0);
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert!(
        serialized["handled_ids"].as_array().expect("serialized handled ids").is_empty()
    );
    assert!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids").is_empty()
    );
}

#[test]
fn peer_sync_reactivation_clears_unpeered_live_completed_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-rejoin-clears-live-completed";
    let entry = PropagationEntryRecord {
        transient_id: "bd".repeat(32),
        destination: "34".repeat(16),
        payload_hex: "34".repeat(24),
        received_at: 1_700_000_903,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_handled_propagation(peer, entry.transient_id.as_str())
        .expect("seed stale completed mark");
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        let record = daemon.transient_peer_record(
            peer.to_string(),
            1_700_000_902,
            Vec::new(),
            None,
            None,
            Some("unpeered".to_string()),
        );
        guard.insert(peer.to_string(), record);
    }

    let result = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("manual"));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("handled ids"),
        Vec::<String>::new()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation_ids(peer)
            .expect("unhandled ids"),
        vec![entry.transient_id.clone()]
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
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn static_peer_activation_clears_unpeered_queue_snapshot() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-static-rejoin-clears-queue";
    let entry = PropagationEntryRecord {
        transient_id: "bc".repeat(32),
        destination: "33".repeat(16),
        payload_hex: "33".repeat(24),
        received_at: 1_700_000_902,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    {
        let mut record = daemon.transient_peer_record(
            peer.to_string(),
            1_700_000_901,
            Vec::new(),
            None,
            None,
            Some("unpeered".to_string()),
        );
        record.restored_handled_ids.push("aa".repeat(32));
        record.restored_unhandled_ids.push("bb".repeat(32));
        record.last_sync_attempt = 1_700_000_900;
        record.next_sync_attempt = 1_700_001_720;
        record.sync_backoff = 720;
        daemon
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .insert(peer.to_string(), record);
    }

    let result = daemon
        .handle_rpc(rpc_request(
            90,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": [peer],
            }),
        ))
        .expect("activate static peer")
        .result
        .expect("propagation enable result");
    assert!(
        result["propagation"]["static_peers"]
            .as_array()
            .expect("static peers")
            .iter()
            .any(|value| value.as_str() == Some(peer))
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 91, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("reactivated static peer row");
    assert_eq!(row["peer_type"].as_str(), Some("static"));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["messages"]["handled_ids"].as_array().expect("handled ids"), &[] as &[JsonValue]);
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let stored = daemon.peers.lock().expect("peers mutex poisoned");
    let record = stored.get(peer).expect("stored peer");
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
