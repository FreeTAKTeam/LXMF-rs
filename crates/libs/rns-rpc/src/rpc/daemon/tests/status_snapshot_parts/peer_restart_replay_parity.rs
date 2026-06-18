fn restored_peer_record(
    peer: &str,
    handled_ids: Vec<String>,
    unhandled_ids: Vec<String>,
) -> PeerRecord {
    let mut record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_960,
        "alive": true,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored peer");
    record.restored_handled_ids = handled_ids;
    record.restored_unhandled_ids = unhandled_ids;
    record
}

#[test]
fn list_peers_replays_restored_unhandled_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-unhandled-replay";
    let entry = PropagationEntryRecord {
        transient_id: "a1".repeat(32),
        destination: "31".repeat(16),
        payload_hex: "31".repeat(24),
        received_at: 1_700_000_961,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    let mixed_case = entry.transient_id.to_ascii_uppercase();
    daemon.peers.lock().expect("peers mutex poisoned").insert(
        peer.to_string(),
        restored_peer_record(
            peer,
            Vec::new(),
            vec![format!("  {mixed_case}  "), entry.transient_id.clone()],
        ),
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 101, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("restored peer row");

    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        daemon.store.list_peer_unhandled_propagation_ids(peer).expect("live unhandled ids"),
        vec![entry.transient_id.clone()]
    );
}

#[test]
fn list_peers_keeps_restored_handled_ids_from_reopening_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-handled-wins";
    let entry = PropagationEntryRecord {
        transient_id: "a2".repeat(32),
        destination: "32".repeat(16),
        payload_hex: "32".repeat(28),
        received_at: 1_700_000_962,
        size_bytes: 28,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    let mixed_case = entry.transient_id.to_ascii_uppercase();
    daemon.peers.lock().expect("peers mutex poisoned").insert(
        peer.to_string(),
        restored_peer_record(
            peer,
            vec![format!(" {mixed_case} ")],
            vec![entry.transient_id.clone()],
        ),
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 102, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("restored peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("live handled ids"),
        vec![entry.transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("live unhandled entries")
            .is_empty()
    );
}

#[test]
fn list_peers_prunes_missing_restored_snapshot_ids_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-missing-prune";
    let handled = PropagationEntryRecord {
        transient_id: "a3".repeat(32),
        destination: "33".repeat(16),
        payload_hex: "33".repeat(20),
        received_at: 1_700_000_963,
        size_bytes: 20,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "a4".repeat(32),
        destination: "34".repeat(16),
        payload_hex: "34".repeat(22),
        received_at: 1_700_000_964,
        size_bytes: 22,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon.peers.lock().expect("peers mutex poisoned").insert(
        peer.to_string(),
        restored_peer_record(
            peer,
            vec!["a5".repeat(32), handled.transient_id.to_ascii_uppercase()],
            vec![
                "a6".repeat(32),
                unhandled.transient_id.clone(),
                unhandled.transient_id.to_ascii_uppercase(),
            ],
        ),
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 103, method: "list_peers".to_string(), params: None })
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
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    assert_eq!(record.restored_handled_ids, vec![handled.transient_id]);
    assert_eq!(record.restored_unhandled_ids, vec![unhandled.transient_id]);
}

#[test]
fn restart_reloads_serialized_restored_queue_snapshot_before_list_peers() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-serialized-reload";
    let handled = PropagationEntryRecord {
        transient_id: "a7".repeat(32),
        destination: "37".repeat(16),
        payload_hex: "37".repeat(26),
        received_at: 1_700_000_965,
        size_bytes: 26,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "a8".repeat(32),
        destination: "38".repeat(16),
        payload_hex: "38".repeat(30),
        received_at: 1_700_000_966,
        size_bytes: 30,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");

    let snapshot: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_967,
        "alive": true,
        "handled_ids": [format!(" {} ", handled.transient_id.to_ascii_uppercase())],
        "unhandled_ids": [
            "a9".repeat(32),
            unhandled.transient_id.to_ascii_uppercase(),
            handled.transient_id.clone(),
            unhandled.transient_id.clone(),
        ],
    }))
    .expect("deserialize peer snapshot");
    let serialized = serde_json::to_value(&snapshot).expect("serialize peer snapshot");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[
            json!("a9".repeat(32)),
            json!(unhandled.transient_id.as_str()),
            json!(handled.transient_id.as_str()),
            json!(unhandled.transient_id.as_str()),
        ]
    );

    let reloaded: PeerRecord =
        serde_json::from_value(serialized).expect("reload serialized peer snapshot");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), reloaded);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 104, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("reloaded peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["offered_bytes"].as_u64(), Some(26));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(30));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("live handled ids"),
        vec![handled.transient_id.clone()]
    );
    assert_eq!(
        daemon.store.list_peer_unhandled_propagation_ids(peer).expect("live unhandled ids"),
        vec![unhandled.transient_id.clone()]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    assert_eq!(record.restored_handled_ids, vec![handled.transient_id]);
    assert_eq!(record.restored_unhandled_ids, vec![unhandled.transient_id]);
}
