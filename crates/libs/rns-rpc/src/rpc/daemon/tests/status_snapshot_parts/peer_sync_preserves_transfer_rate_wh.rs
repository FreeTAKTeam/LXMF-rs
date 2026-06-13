#[test]
fn peer_sync_preserves_transfer_rate_when_no_offers_remain_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4c);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }

    let entry = PropagationEntryRecord {
        transient_id: "dc".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "30".repeat(48),
        received_at: 1_700_000_624,
        size_bytes: 48,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with transfer");
    let expected_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x30; 48]])).expect("pack sync resource").len();

    let result = daemon
        .handle_rpc(rpc_request(65, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("peer sync without offers")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(0));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(expected_resource_bytes as u64));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 66, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(expected_resource_bytes as u64));
}

#[test]
fn peer_sync_preserves_transfer_rate_when_offers_are_skipped_or_transfer_limited() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4d);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }

    let handled = PropagationEntryRecord {
        transient_id: "d8".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "26".repeat(40),
        received_at: 1_700_000_620,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), handled.transient_id.as_str())
        .expect("mark handled unhandled");
    daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with transfer");
    let first_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x26; 40]])).expect("pack sync resource").len();

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(24 + 40 + 16);
    }
    let skipped = PropagationEntryRecord {
        transient_id: "d9".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "27".repeat(40),
        received_at: 1_700_000_621,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&skipped).expect("store skipped entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), skipped.transient_id.as_str())
        .expect("mark skipped unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            65,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with skipped offer")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(first_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(first_resource_bytes as u64));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 66, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(first_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(first_resource_bytes as u64));

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = None;
        record.propagation_sync_limit = Some(1_000);
        record.next_sync_attempt = 0;
        record.sync_backoff = 0;
    }
    let second_handled = PropagationEntryRecord {
        transient_id: "da".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "28".repeat(32),
        received_at: 1_700_000_622,
        size_bytes: 32,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&second_handled).expect("store second handled entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            second_handled.transient_id.as_str(),
        )
        .expect("mark second handled unhandled");
    daemon
        .handle_rpc(rpc_request(
            67,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with second transfer");
    let second_resource_bytes = rmp_serde::to_vec(&(1.0_f64, vec![vec![0x27; 40], vec![0x28; 32]]))
        .expect("pack sync resource")
        .len();

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = Some(80);
        record.propagation_sync_limit = Some(1_000);
    }
    let transfer_limited = PropagationEntryRecord {
        transient_id: "db".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "29".repeat(100),
        received_at: 1_700_000_623,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon
        .store
        .upsert_propagation_entry(&transfer_limited)
        .expect("store transfer limited entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            transfer_limited.transient_id.as_str(),
        )
        .expect("mark transfer limited unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            68,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with transfer-limited offer")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(second_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(second_resource_bytes as u64));
}

#[test]
fn peer_sync_reports_transferred_propagation_messages() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([3u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d3".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "21".repeat(24),
        received_at: 1_700_000_614,
        size_bytes: 24,
        stamp_value: Some(11),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer_id.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let messages = result["propagation"]["messages"].as_array().expect("propagation messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["transient_id"].as_str(), Some(entry.transient_id.as_str()));
    assert_eq!(messages[0]["destination"].as_str(), Some(entry.destination.as_str()));
    assert_eq!(messages[0]["payload_hex"].as_str(), Some(entry.payload_hex.as_str()));
    assert_eq!(messages[0]["received_at"].as_i64(), Some(entry.received_at));
    assert_eq!(messages[0]["size_bytes"].as_u64(), Some(entry.size_bytes));
    assert_eq!(messages[0]["stamp_value"].as_u64(), Some(11));
}

#[test]
fn peer_sync_invalid_response_payload_does_not_partially_mark_transferred_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-invalid-response-payload";
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.propagation_stamp_cost = Some(0);
    }

    let valid = PropagationEntryRecord {
        transient_id: "a1".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: None,
    };
    let invalid = PropagationEntryRecord {
        transient_id: "a2".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "not-hex".to_string(),
        received_at: 1_700_000_618,
        size_bytes: 7,
        stamp_value: None,
    };
    for entry in [&valid, &invalid] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
            .expect("mark unhandled");
        daemon.record_peer_queue_unhandled_id(peer, entry.transient_id.as_str());
    }

    let err = daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer,
                "wanted_ids": [valid.transient_id.as_str(), invalid.transient_id.as_str()],
            }),
        ))
        .expect_err("invalid response payload should fail peer sync");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("invalid propagation payload hex"),
        "unexpected error: {err}"
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer)
        .expect("pending propagation");
    assert_eq!(pending, vec![valid.clone(), invalid.clone()]);
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer)
            .expect("handled propagation")
            .is_empty()
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("peer record");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert!(
        serialized["handled_ids"]
            .as_array()
            .expect("serialized handled ids")
            .is_empty()
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(valid.transient_id.as_str()), json!(invalid.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_true_response_invalid_payload_does_not_partially_mark_transferred_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-invalid-true-response-payload";
    daemon
        .handle_rpc(rpc_request(65, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.propagation_stamp_cost = Some(0);
    }

    let valid = PropagationEntryRecord {
        transient_id: "a3".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "24".repeat(24),
        received_at: 1_700_000_619,
        size_bytes: 24,
        stamp_value: None,
    };
    let invalid = PropagationEntryRecord {
        transient_id: "a4".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "not-hex".to_string(),
        received_at: 1_700_000_620,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&valid, &invalid] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
            .expect("mark unhandled");
        daemon.record_peer_queue_unhandled_id(peer, entry.transient_id.as_str());
    }

    let err = daemon
        .handle_rpc(rpc_request(
            66,
            "peer_sync",
            json!({
                "peer": peer,
                "wanted_ids": true,
            }),
        ))
        .expect_err("invalid true response payload should fail peer sync");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("invalid propagation payload hex"),
        "unexpected error: {err}"
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer)
        .expect("pending propagation");
    assert_eq!(pending, vec![valid.clone(), invalid.clone()]);
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer)
            .expect("handled propagation")
            .is_empty()
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("peer record");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert!(
        serialized["handled_ids"]
            .as_array()
            .expect("serialized handled ids")
            .is_empty()
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(valid.transient_id.as_str()), json!(invalid.transient_id.as_str())]
    );
}
