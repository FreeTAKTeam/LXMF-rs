#[test]
fn peer_sync_marks_entries_above_transfer_limit_handled_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-transfer-oversize" })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-oversize").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.alive = false;
        peer.sync_backoff = 720;
        peer.next_sync_attempt = 1_700_000_720;
    }

    let oversized = PropagationEntryRecord {
        transient_id: "c3".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_612,
        size_bytes: 100,
        stamp_value: None,
    };
    let oversized_id = oversized.transient_id.clone();
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-transfer-oversize", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": "peer-transfer-oversize" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"].as_array().expect("transfer limited ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert!(result["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids("peer-transfer-oversize")
        .expect("handled ids");
    assert_eq!(handled, vec![oversized_id.clone()]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-transfer-oversize")
        .expect("pending propagation");
    assert!(pending.is_empty());

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(event.payload["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(
        event.payload["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("event transfer limited ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_does_not_retry_transfer_limited_entries_when_limit_increases_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-transfer-retry" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-retry").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
    }

    let oversized = PropagationEntryRecord {
        transient_id: "c4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_613,
        size_bytes: 100,
        stamp_value: None,
    };
    let oversized_id = oversized.transient_id.clone();
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-transfer-retry", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let limited = daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": "peer-transfer-retry" })))
        .expect("limited peer sync")
        .result
        .expect("limited peer sync result");
    assert_eq!(limited["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(limited["messages"]["offered"].as_u64(), Some(0));

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-retry").expect("peer record");
        peer.propagation_transfer_limit = Some(200);
        peer.propagation_sync_limit = Some(1_000);
    }

    let retried = daemon
        .handle_rpc(rpc_request(
            62,
            "peer_sync",
            json!({
                "peer": "peer-transfer-retry",
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("retried peer sync")
        .result
        .expect("retried peer sync result");
    assert_eq!(retried["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(retried["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(retried["propagation"]["transfer_limited"].as_u64(), Some(0));
    assert_eq!(
        retried["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[] as &[JsonValue]
    );
    assert!(
        retried["propagation"]["messages"].as_array().expect("messages").is_empty()
    );
    assert_eq!(retried["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(retried["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-transfer-retry")
            .expect("handled ids"),
        vec![oversized_id]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-transfer-retry")
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_applies_request_transfer_limit_without_persisting_it() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-request-limit" })))
        .expect("initial peer sync");

    let oversized = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_613,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-request-limit", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-request-limit",
                "transfer_limit_kb": 0.08,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["transfer_limit"].as_u64(), Some(80));
    assert!(result["sync_limit"].is_null());

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(80));
    assert!(event.payload["sync_limit"].is_null());
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(80));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 62, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-request-limit"))
        .expect("peer row");
    assert_eq!(row["propagation_transfer_limit"], JsonValue::Null);
}

#[test]
fn peer_sync_accepts_string_transfer_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-string-limit" })))
        .expect("initial peer sync");

    let oversized = PropagationEntryRecord {
        transient_id: "d6".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_615,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-string-limit", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-string-limit",
                "transfer_limit_kb": "0.08",
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["transfer_limit"].as_u64(), Some(80));
}

#[test]
fn peer_sync_request_transfer_limit_does_not_loosen_peer_limit() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-strict-limit" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-strict-limit").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
    }

    let oversized = PropagationEntryRecord {
        transient_id: "d5".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_614,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-strict-limit", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-strict-limit",
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
}

#[test]
fn postponed_peer_sync_reports_request_transfer_limit() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-postponed-limit" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-postponed-limit").expect("peer record");
        peer.next_sync_attempt = i64::MAX;
    }

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-postponed-limit",
                "transfer_limit_kb": 0.08,
            }),
        ))
        .expect("postponed peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert!(result["propagation"]["sync_limit"].is_null());
    assert_eq!(result["transfer_limit"].as_u64(), Some(80));
    assert!(result["sync_limit"].is_null());

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("postponed peer sync event");
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(80));
    assert!(event.payload["sync_limit"].is_null());
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(80));
}

#[test]
fn postponed_peer_sync_backoff_preserves_alive_when_attempt_matches_last_heard_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-backoff-equal-heard" })))
        .expect("initial peer sync");
    let record = {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-backoff-equal-heard").expect("peer record");
        peer.alive = true;
        peer.last_seen = 1_700_001_000;
        peer.last_sync_attempt = 1_700_000_900;
        peer.next_sync_attempt = 1_700_001_720;
        peer.clone()
    };

    let result = daemon
        .postponed_peer_sync_response(
            61,
            &record,
            1_700_001_000,
            "backoff",
            Some(80),
            None,
        )
        .result
        .expect("postponed peer sync result");

    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["last_sync_attempt"].as_i64(), Some(1_700_001_000));
    assert_eq!(result["last_heard"].as_i64(), Some(1_700_001_000));
    assert_eq!(result["alive"].as_bool(), Some(true));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 62, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-backoff-equal-heard"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
}
