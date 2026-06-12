#[test]
fn peer_sync_retryable_offer_response_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-local-retry-snapshot";
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
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
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer,
                "wanted_ids": 0xf0,
            }),
        ))
        .expect("identity-required response should preserve peer queue for retry")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["reason"].as_str(), Some("identity_required"));
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
fn peer_sync_rejects_transfer_limited_wanted_ids_without_mutating_queue() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-limited-wanted" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-limited-wanted").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
    }
    let pending = PropagationEntryRecord {
        transient_id: "a9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(100),
        received_at: 1_700_000_608,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-limited-wanted", pending.transient_id.as_str())
        .expect("mark unhandled");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-limited-wanted",
                "wanted_ids": [pending.transient_id.as_str()],
            }),
        ))
        .expect_err("transfer-limited wanted id should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("current peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-limited-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-limited-wanted")
            .expect("pending propagation"),
        vec![pending]
    );
}

#[test]
fn list_peers_top_level_message_counters_match_python_sync_accounting() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xaf);
    let wanted = PropagationEntryRecord {
        transient_id: "af".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_609,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "b0".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(30),
        received_at: 1_700_000_610,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["offered"].as_u64(), Some(2));
    assert_eq!(row["outgoing"].as_u64(), Some(1));
    assert_eq!(row["incoming"].as_u64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));
}

#[test]
fn peer_sync_result_reports_cumulative_acceptance_rate_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x43);
    let first = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_611,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&first).expect("store first entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), first.transient_id.as_str())
        .expect("mark first unhandled");
    daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let wanted = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_612,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            58,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("second peer sync")
        .result
        .expect("second peer sync result");
    assert_eq!(result["messages"]["offered"].as_u64(), Some(3));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(2));
    assert!(
        result["acceptance_rate"]
            .as_f64()
            .is_some_and(|value| (value - (2.0 / 3.0)).abs() < f64::EPSILON)
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(3));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(2));
    assert!(
        event.payload["acceptance_rate"]
            .as_f64()
            .is_some_and(|value| (value - (2.0 / 3.0)).abs() < f64::EPSILON)
    );
}

#[test]
fn peer_sync_stores_cumulative_acceptance_rate_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xc7);
    let transferred = PropagationEntryRecord {
        transient_id: "c7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "17".repeat(24),
        received_at: 1_700_000_616,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon
        .store
        .upsert_propagation_entry(&transferred)
        .expect("store transferred entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), transferred.transient_id.as_str())
        .expect("mark transferred unhandled");
    daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("initial peer sync");

    let skipped = PropagationEntryRecord {
        transient_id: "c8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "18".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon
        .store
        .upsert_propagation_entry(&skipped)
        .expect("store skipped entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), skipped.transient_id.as_str())
        .expect("mark skipped unhandled");
    daemon
        .handle_rpc(rpc_request(
            62,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": false,
            }),
        ))
        .expect("no-transfer offer response");

    let stored = daemon.peers.lock().expect("peers mutex poisoned");
    let record = stored.get(peer.as_str()).expect("stored peer");
    assert_eq!(record.offered, 2);
    assert_eq!(record.outgoing, 1);
    assert!(
        (record.acceptance_rate - 0.5).abs() < f64::EPSILON,
        "stored acceptance rate should be lifetime outgoing/offered"
    );
}

#[test]
fn peer_sync_persists_cumulative_acceptance_rate_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x44);
    let first = PropagationEntryRecord {
        transient_id: "44".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_611,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&first).expect("store first entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), first.transient_id.as_str())
        .expect("mark first unhandled");
    daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("initial peer sync");

    let wanted = PropagationEntryRecord {
        transient_id: "45".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_612,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "46".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            58,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("second peer sync");

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer.as_str()).expect("peer record");
    assert_eq!(record.offered, 3);
    assert_eq!(record.outgoing, 2);
    assert!(
        (record.acceptance_rate - (2.0 / 3.0)).abs() < f64::EPSILON,
        "stored acceptance rate should remain cumulative, got {}",
        record.acceptance_rate
    );
}
