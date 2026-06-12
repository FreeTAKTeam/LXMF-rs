#[test]
fn peer_sync_postpones_stamped_offers_until_peering_key_is_ready() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-missing-peering-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-missing-peering-key").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ec".repeat(32),
        destination: "1c".repeat(16),
        payload_hex: "1c".repeat(20),
        received_at: 1_700_000_618,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-missing-peering-key", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-missing-peering-key" })))
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
        .list_peer_unhandled_propagation("peer-missing-peering-key")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_records_queued_existing_entries_in_peer_record_snapshot() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-queue-snapshot";
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer_record = peers.get_mut(peer).expect("peer record");
        peer_record.propagation_sync_limit = Some(1_000);
        peer_record.propagation_stamp_cost = Some(1);
        peer_record.propagation_stamp_cost_flexibility = Some(1);
        peer_record.peering_cost = Some(1);
        peer_record.peering_key_value = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_619,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer })))
        .expect("peering-key-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(
        serialized["handled_ids"].as_array().expect("serialized handled ids").is_empty()
    );
}

#[test]
fn peer_sync_records_preexisting_live_queue_marks_in_peer_record_snapshot() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-preexisting-live-queue-snapshot";
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer_record = peers.get_mut(peer).expect("peer record");
        peer_record.propagation_sync_limit = Some(1_000);
        peer_record.propagation_stamp_cost = Some(1);
        peer_record.propagation_stamp_cost_flexibility = Some(1);
        peer_record.peering_cost = Some(1);
        peer_record.peering_key_value = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(20),
        received_at: 1_700_000_620,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("seed live queue mark");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer })))
        .expect("peering-key-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(
        serialized["handled_ids"].as_array().expect("serialized handled ids").is_empty()
    );
}

#[test]
fn peer_sync_uses_restored_python_peering_key_value() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-python-key";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_618,
        "alive": true,
        "propagation_transfer_limit": 1,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 1,
        "propagation_stamp_cost_flexibility": 1,
        "peering_cost": 1,
        "peering_key": ["opaque-python-key", 1],
        "sync_strategy": 1,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored Python peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let entry = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_619,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": peer })))
        .expect("restored-key peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["sync_strategy"].as_u64(), Some(1));
    assert_eq!(result["peering_key"].as_u64(), Some(1));
    assert_eq!(result["peering_key_status"].as_str(), Some("ready"));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    let event = events.iter().find(|event| event.event_type == "peer_sync").expect("peer event");
    assert_eq!(event.payload["sync_strategy"].as_u64(), Some(1));
}

#[test]
fn peer_sync_restored_python_float_costs_drive_peering_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-float-costs";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_618,
        "alive": true,
        "propagation_transfer_limit": 1,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 1.9,
        "propagation_stamp_cost_flexibility": 1.1,
        "peering_cost": 1.0,
        "peering_key": ["opaque-python-key", 1],
        "sync_strategy": 1,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored Python peer with float costs");
    assert_eq!(record.propagation_stamp_cost, Some(1));
    assert_eq!(record.propagation_stamp_cost_flexibility, Some(1));
    assert_eq!(record.peering_cost, Some(1));
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1e".repeat(20),
        received_at: 1_700_000_620,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": peer })))
        .expect("restored float-cost peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["peering_key"].as_u64(), Some(1));
    assert_eq!(result["peering_key_status"].as_str(), Some("ready"));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_clears_restored_python_peering_key_below_cost_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-low-key";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_618,
        "alive": true,
        "propagation_transfer_limit": 1,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 1,
        "propagation_stamp_cost_flexibility": 1,
        "peering_cost": 2,
        "peering_key": ["opaque-python-key", 1],
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored Python peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "1e".repeat(20),
        received_at: 1_700_000_620,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": peer })))
        .expect("low-key peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["peering_key_status"].as_str(), Some("not_ready"));

    let stored = daemon.peers.lock().expect("peers mutex poisoned");
    let record = stored.get(peer).expect("stored peer");
    assert_eq!(
        record.peering_key_value, None,
        "Python peering_key_ready clears keys below peering_cost"
    );
}

#[test]
fn peer_sync_restores_python_peer_record_queue_marks_for_existing_entries_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-python-queue";
    let handled = PropagationEntryRecord {
        transient_id: "e1".repeat(32),
        destination: "11".repeat(16),
        payload_hex: "11".repeat(20),
        received_at: 1_700_000_621,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(20),
        received_at: 1_700_000_622,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");

    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_620,
        "alive": true,
        "propagation_transfer_limit": 1,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 1,
        "propagation_stamp_cost_flexibility": 1,
        "peering_cost": 1,
        "handled_ids": [
            handled.transient_id.to_ascii_uppercase(),
            handled.transient_id,
            "fa".repeat(32)
        ],
        "unhandled_ids": [
            unhandled.transient_id.to_ascii_uppercase(),
            unhandled.transient_id,
            "fb".repeat(32)
        ],
    }))
    .expect("deserialize restored Python peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer })))
        .expect("restored queue peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 58, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("restored peer row");

    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!("e1".repeat(32))]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!("e2".repeat(32))]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!("e1".repeat(32))]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!("e2".repeat(32))]
    );
}
