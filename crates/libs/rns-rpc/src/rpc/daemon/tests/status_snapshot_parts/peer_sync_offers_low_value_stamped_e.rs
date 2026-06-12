#[test]
fn peer_sync_offers_low_value_stamped_entries_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([5u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d5".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: Some(5),
    };
    let accepted = PropagationEntryRecord {
        transient_id: "d6".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "24".repeat(24),
        received_at: 1_700_000_618,
        size_bytes: 24,
        stamp_value: Some(6),
    };
    for entry in [&low_value, &accepted] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer_id.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected_bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(accepted.transient_id.as_str()), json!(low_value.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
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
    assert_eq!(event.payload["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(event.payload["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["rejected_ids"].as_array().expect("event rejected ids"),
        &[] as &[JsonValue]
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer_id.as_str())
        .expect("pending propagation");
    assert!(pending.is_empty());
    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer_id.as_str())
        .expect("handled propagation");
    assert_eq!(handled, vec![low_value.transient_id, accepted.transient_id]);
}

#[test]
fn peer_sync_postpones_low_value_stamped_entries_before_peering_key_gate_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": "peer-low-value-no-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-low-value-no-key").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d3".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: Some(5),
    };
    daemon.store.upsert_propagation_entry(&low_value).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-low-value-no-key", low_value.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": "peer-low-value-no-key" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );

    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-low-value-no-key")
            .expect("pending propagation"),
        vec![low_value]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-low-value-no-key")
            .expect("handled propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_transfer_limits_low_value_stamped_entries_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": "peer-low-value-oversized" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-low-value-oversized").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = 0;
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(100),
        received_at: 1_700_000_617,
        size_bytes: 100,
        stamp_value: Some(5),
    };
    daemon.store.upsert_propagation_entry(&low_value).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-low-value-oversized",
            low_value.transient_id.as_str(),
        )
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": "peer-low-value-oversized" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected_bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer-limited ids"),
        &[json!(low_value.transient_id.as_str())]
    );

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-low-value-oversized")
            .expect("pending propagation")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-low-value-oversized")
            .expect("handled propagation"),
        vec![low_value.transient_id]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 65, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-low-value-oversized"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_postpones_low_value_stamped_entries_with_unconfigured_peering_cost_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": "peer-low-value-no-cost" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-low-value-no-cost").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = None;
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d0".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: Some(5),
    };
    daemon.store.upsert_propagation_entry(&low_value).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-low-value-no-cost", low_value.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": "peer-low-value-no-cost" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["peering_key_status"].as_str(), Some("unconfigured"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );

    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-low-value-no-cost")
            .expect("pending propagation"),
        vec![low_value]
    );
}

#[test]
fn peer_sync_result_and_event_report_message_accounting() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([4u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "22".repeat(24),
        received_at: 1_700_000_616,
        size_bytes: 24,
        stamp_value: Some(12),
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
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(24));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[] as &[JsonValue]
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
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(24));
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[] as &[JsonValue]
    );
}
