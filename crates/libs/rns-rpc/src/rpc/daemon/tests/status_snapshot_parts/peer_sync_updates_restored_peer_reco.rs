#[test]
fn peer_sync_updates_restored_peer_record_queue_ids_after_wants_none_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-python-queue-response";
    let entry = PropagationEntryRecord {
        transient_id: "e3".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(20),
        received_at: 1_700_000_623,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");

    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_620,
        "alive": true,
        "propagation_transfer_limit": 1,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 1,
        "propagation_stamp_cost_flexibility": 1,
        "peering_cost": 1,
        "peering_key": [null, 1],
        "handled_ids": [],
        "unhandled_ids": [entry.transient_id.clone()],
    }))
    .expect("deserialize restored Python peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let result = daemon
        .handle_rpc(rpc_request(58, "peer_sync", json!({ "peer": peer, "wanted_ids": [] })))
        .expect("restored queue peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(
        result["messages"]["unhandled_ids"]
            .as_array()
            .expect("result unhandled ids")
            .is_empty()
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids").is_empty()
    );
}

#[test]
fn empty_peer_sync_checks_peering_key_before_no_unhandled_shortcut_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-empty-key-policy";
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.propagation_stamp_cost = Some(1);
        record.propagation_stamp_cost_flexibility = Some(0);
        record.peering_cost = Some(1);
        record.peering_key_value = None;
        record.sync_backoff = 0;
        record.next_sync_attempt = 0;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": peer })))
        .expect("empty peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["peering_key_status"].as_str(), Some("not_ready"));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("postponed peer sync event");
    assert_eq!(event.payload["postponed"].as_bool(), Some(true));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
}

#[test]
fn peer_sync_transfer_limits_oversized_stamped_entries_before_peering_key_gate() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-key-limit-first" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-key-limit-first").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let oversized = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(100),
        received_at: 1_700_000_621,
        size_bytes: 100,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-key-limit-first", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-key-limit-first" })))
        .expect("transfer-limited peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["postpone_reason"], JsonValue::Null);
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer limited ids"),
        &[json!(oversized.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-key-limit-first")
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-key-limit-first")
            .expect("handled ids"),
        vec![oversized.transient_id.clone()]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("peer-key-limit-first").expect("peer record");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(oversized.transient_id.as_str())]
    );
    assert!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids").is_empty()
    );
}

#[test]
fn peer_sync_transfer_limits_wants_none_oversized_entries_before_peering_key_gate() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-key-limit-wants-none" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-key-limit-wants-none").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let oversized = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "1e".repeat(100),
        received_at: 1_700_000_623,
        size_bytes: 100,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-key-limit-wants-none",
            oversized.transient_id.as_str(),
        )
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            54,
            "peer_sync",
            json!({
                "peer": "peer-key-limit-wants-none",
                "wanted_ids": false,
            }),
        ))
        .expect("transfer-limited offer response")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer limited ids"),
        &[json!(oversized.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-key-limit-wants-none")
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-key-limit-wants-none")
            .expect("handled ids"),
        vec![oversized.transient_id]
    );
}

#[test]
fn peer_sync_checks_peering_key_before_sync_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-key-sync-limit-first" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-key-sync-limit-first").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let skipped = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_622,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&skipped).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-key-sync-limit-first", skipped.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-key-sync-limit-first" })))
        .expect("sync-limited peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("peering_key")
    );
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["remaining_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[] as &[JsonValue]
    );

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-key-sync-limit-first")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, skipped.transient_id);
}

#[test]
fn peer_sync_postpones_unstamped_offers_until_peering_key_is_ready() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-unstamped-missing-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unstamped-missing-key").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "1e".repeat(20),
        received_at: 1_700_000_620,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unstamped-missing-key", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-unstamped-missing-key" })))
        .expect("peering-key-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-unstamped-missing-key")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_transfers_unstamped_offers_when_stamp_cost_zero_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-zero-stamp-cost" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-zero-stamp-cost").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(0);
        peer.propagation_stamp_cost_flexibility = Some(0);
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ec".repeat(32),
        destination: "1c".repeat(16),
        payload_hex: "1c".repeat(20),
        received_at: 1_700_000_623,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-zero-stamp-cost", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-zero-stamp-cost" })))
        .expect("zero-stamp peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_ne!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"], JsonValue::Null);
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["peering_key_status"].as_str(), Some("unconfigured"));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-zero-stamp-cost")
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-zero-stamp-cost")
            .expect("handled ids"),
        vec![entry.transient_id]
    );
}
