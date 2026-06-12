#[test]
fn static_peer_activation_clears_unpeered_live_completed_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-static-rejoin-clears-live-completed";
    let entry = PropagationEntryRecord {
        transient_id: "be".repeat(32),
        destination: "35".repeat(16),
        payload_hex: "35".repeat(24),
        received_at: 1_700_000_904,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_handled_propagation(peer, entry.transient_id.as_str())
        .expect("seed stale completed mark");
    {
        let mut record = daemon.transient_peer_record(
            peer.to_string(),
            1_700_000_903,
            Vec::new(),
            None,
            None,
            Some("unpeered".to_string()),
        );
        record.restored_handled_ids.push(entry.transient_id.clone());
        daemon
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .insert(peer.to_string(), record);
    }

    daemon
        .handle_rpc(rpc_request(
            90,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": [peer],
            }),
        ))
        .expect("activate static peer");

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
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_matches_existing_peer_queue_case_insensitively_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let stored_peer = "Ab".repeat(16);
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .accept_announce_with_metadata(
            stored_peer.clone(),
            1_700_000_930,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(1),
            Some(Some(1)),
            Some(Some(1)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept mixed-case propagation peer");
    let entry = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "44".repeat(24),
        received_at: 1_700_000_931,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer.as_str(), entry.transient_id.as_str())
        .expect("mark mixed-case peer unhandled");

    let result = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": request_peer })))
        .expect("peer sync with lowercase id")
        .result
        .expect("peer sync result");

    assert_eq!(result["peer"].as_str(), Some(stored_peer.as_str()));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(stored_peer.as_str())
            .expect("mixed-case handled ids"),
        vec![entry.transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer.as_str())
            .expect("mixed-case unhandled")
            .is_empty()
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 91, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["peer"].as_str(), Some(stored_peer.as_str()));
    assert_eq!(rows[0]["messages"]["handled_ids"].as_array().expect("handled ids"), &[
        json!(entry.transient_id.as_str()),
    ]);
}

#[test]
fn peer_queue_unhandled_snapshot_preserves_case_insensitive_completed_mark_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Completed-Mixed-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(91, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "de".repeat(32),
        destination: "24".repeat(16),
        payload_hex: "24".repeat(24),
        received_at: 1_700_000_940,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_transfer_limited_propagation(stored_peer, entry.transient_id.as_str())
        .expect("mark transfer limited");

    daemon.record_peer_queue_unhandled_id(request_peer.as_str(), entry.transient_id.as_str());

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_queue_unhandled_snapshot_respects_case_variant_completed_mark_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Completed-Replay-Mixed";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(91, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "28".repeat(16),
        payload_hex: "28".repeat(24),
        received_at: 1_700_000_941,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_transfer_limited_propagation(request_peer.as_str(), entry.transient_id.as_str())
        .expect("mark case-variant transfer limited");

    daemon.record_peer_queue_unhandled_id(stored_peer, entry.transient_id.as_str());

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_queue_snapshot_helpers_canonicalize_transient_ids_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Snapshot-Canonical-Ids";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(91, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "df".repeat(32),
        destination: "25".repeat(16),
        payload_hex: "25".repeat(24),
        received_at: 1_700_000_945,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    let request_transient_id = format!("  {}  ", entry.transient_id.to_ascii_uppercase());

    daemon.record_peer_queue_unhandled_id(request_peer.as_str(), request_transient_id.as_str());
    {
        let peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get(stored_peer).expect("stored peer");
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

    daemon.record_peer_queue_handled_id(request_peer.as_str(), request_transient_id.as_str());
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_completed_mark_helpers_write_stored_peer_case_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Mark-Mixed-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(92, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let transferred = PropagationEntryRecord {
        transient_id: "e1".repeat(32),
        destination: "26".repeat(16),
        payload_hex: "26".repeat(24),
        received_at: 1_700_000_950,
        size_bytes: 24,
        stamp_value: None,
    };
    let received = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "27".repeat(16),
        payload_hex: "27".repeat(28),
        received_at: 1_700_000_951,
        size_bytes: 28,
        stamp_value: None,
    };
    for entry in [&transferred, &received] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
    }

    daemon
        .record_peer_transferred_propagation(request_peer.as_str(), transferred.transient_id.as_str())
        .expect("record transferred");
    daemon
        .record_peer_received_propagation(request_peer.as_str(), received.transient_id.as_str())
        .expect("record received");

    assert!(
        daemon
            .has_peer_completed_propagation_mark(stored_peer, transferred.transient_id.as_str())
            .expect("transferred mark"),
        "transferred mark should be visible under stored peer case"
    );
    assert!(
        daemon
            .has_peer_completed_propagation_mark(stored_peer, received.transient_id.as_str())
            .expect("received mark"),
        "received mark should be visible under stored peer case"
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(stored_peer)
            .expect("stored peer handled ids"),
        vec![transferred.transient_id.clone(), received.transient_id.clone()]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(transferred.transient_id.as_str()), json!(received.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_activation_snapshots_preexisting_completed_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-late-completed-snapshot";
    let entry = PropagationEntryRecord {
        transient_id: "e4".repeat(32),
        destination: "28".repeat(16),
        payload_hex: "28".repeat(24),
        received_at: 1_700_000_952,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .record_peer_transferred_propagation(peer, entry.transient_id.as_str())
        .expect("record transfer before peer activation");

    daemon.record_propagation_offer_peer(peer).expect("activate propagation peer");

    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("handled ids"),
        vec![entry.transient_id.clone()]
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("peer record");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}
