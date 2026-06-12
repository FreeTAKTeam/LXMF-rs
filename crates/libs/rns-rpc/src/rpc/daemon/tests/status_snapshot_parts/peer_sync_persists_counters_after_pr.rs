#[test]
fn peer_sync_persists_counters_after_propagation_entries_are_purged_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb4);
    let wanted = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "15".repeat(24),
        received_at: 1_700_000_614,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "b5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "16".repeat(24),
        received_at: 1_700_000_615,
        size_bytes: 24,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(
                peer.as_str(),
                entry.transient_id.as_str(),
            )
            .expect("mark unhandled");
    }

    let synced = daemon
        .handle_rpc(rpc_request(
            59,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(synced["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(synced["messages"]["outgoing"].as_u64(), Some(1));

    let purged = daemon
        .store
        .purge_propagation_entries_for_destination(
            wanted.destination.as_str(),
            &[wanted.transient_id.clone(), already_known.transient_id.clone()],
        )
        .expect("purge propagation entries");
    assert_eq!(purged, 2);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 60, method: "list_peers".to_string(), params: None })
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
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(row["offered"].as_u64(), Some(2));
    assert_eq!(row["outgoing"].as_u64(), Some(1));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));
}

#[test]
fn peer_sync_drops_stale_unhandled_propagation_marks() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-stale-propagation" })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-stale-propagation", "fa".repeat(32).as_str())
        .expect("mark stale unhandled");

    let before = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let before_row = before["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-stale-propagation"))
        .expect("peer row");
    assert_eq!(before_row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        before_row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        before_row["unhandled_ids"].as_array().expect("top-level unhandled ids"),
        &[] as &[JsonValue]
    );

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": "peer-stale-propagation" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));

    let after = daemon
        .handle_rpc(RpcRequest { id: 58, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let after_row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-stale-propagation"))
        .expect("peer row");
    assert_eq!(after_row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(after_row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(
        after_row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        after_row["unhandled_ids"].as_array().expect("top-level unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_sync_prunes_stale_unhandled_peer_record_snapshot_ids() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-stale-snapshot";
    let stale_id = "fc".repeat(32);
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, stale_id.as_str())
        .expect("mark stale unhandled");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_unhandled_ids.push(stale_id.clone());
    }

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert!(
        result["messages"]["unhandled_ids"]
            .as_array()
            .expect("result unhandled ids")
            .is_empty()
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids").is_empty()
    );
    assert!(
        serialized["handled_ids"].as_array().expect("serialized handled ids").is_empty()
    );
}

#[test]
fn list_peers_ignores_stale_handled_propagation_marks() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-stale-handled" })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_handled_propagation("peer-stale-handled", "fb".repeat(32).as_str())
        .expect("mark stale handled");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-stale-handled"))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(row["messages"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        row["handled_ids"].as_array().expect("top-level handled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_sync_prunes_stale_handled_peer_record_snapshot_ids() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-stale-handled-snapshot";
    let stale_id = "fd".repeat(32);
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_handled_propagation(peer, stale_id.as_str())
        .expect("mark stale handled");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.push(stale_id.clone());
    }

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids").is_empty()
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
    assert!(
        serialized["handled_ids"].as_array().expect("serialized handled ids").is_empty()
    );
    assert!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids").is_empty()
    );
}

#[test]
fn peer_sync_prunes_case_variant_stale_live_queue_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Stale-Live-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    let stale_unhandled_id = "fe".repeat(32);
    let stale_handled_id = "ff".repeat(32);
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": stored_peer })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_unhandled_propagation(request_peer.as_str(), stale_unhandled_id.as_str())
        .expect("mark case-variant stale unhandled");
    daemon
        .store
        .mark_peer_handled_propagation(request_peer.as_str(), stale_handled_id.as_str())
        .expect("mark case-variant stale handled");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_unhandled_ids.push(stale_unhandled_id.clone());
        record.restored_handled_ids.push(stale_handled_id.clone());
    }

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer"].as_str(), Some(stored_peer));
    assert!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids").is_empty()
    );
    assert!(
        result["messages"]["unhandled_ids"]
            .as_array()
            .expect("result unhandled ids")
            .is_empty()
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert!(
        serialized["handled_ids"].as_array().expect("serialized handled ids").is_empty()
    );
    assert!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids").is_empty()
    );
    drop(peers);

    assert!(
        daemon
            .store
            .remove_stale_peer_unhandled_propagation_ids(request_peer.as_str())
            .expect("case-variant stale unhandled cleanup")
            .is_empty()
    );
    assert!(
        daemon
            .store
            .remove_stale_peer_completed_propagation_ids(request_peer.as_str())
            .expect("case-variant stale completed cleanup")
            .is_empty()
    );
}

#[test]
fn peer_sync_applies_per_peer_propagation_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x44);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some((24 + 20 + 32 + 16 + 1) as u32);
    }

    let small = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let large = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(100),
        received_at: 1_700_000_609,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&small).expect("store small entry");
    daemon.store.upsert_propagation_entry(&large).expect("store large entry");
    for entry in [&small, &large] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({
                "peer": peer,
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("budgeted peer sync");

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![small.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![large]);
}
