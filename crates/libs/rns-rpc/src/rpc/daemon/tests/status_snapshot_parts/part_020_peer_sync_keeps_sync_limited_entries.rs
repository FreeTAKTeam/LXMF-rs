#[test]
fn peer_sync_keeps_sync_limited_entries_queued_when_peer_wants_none() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb1);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(80);
    }
    let offered = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(10),
        received_at: 1_700_000_611,
        size_bytes: 10,
        stamp_value: None,
    };
    let skipped = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(40),
        received_at: 1_700_000_612,
        size_bytes: 40,
        stamp_value: None,
    };
    for entry in [&offered, &skipped] {
        daemon.store.upsert_propagation_entry(entry).expect("store entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(offered.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(skipped.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![offered.transient_id]
    );
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![skipped]);
}

#[test]
fn peer_sync_skips_policy_ready_entries_by_cumulative_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x48);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(80);
    }
    let transferable = PropagationEntryRecord {
        transient_id: "a6".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(10),
        received_at: 1_700_000_611,
        size_bytes: 10,
        stamp_value: None,
    };
    let skipped = PropagationEntryRecord {
        transient_id: "a7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(40),
        received_at: 1_700_000_612,
        size_bytes: 40,
        stamp_value: Some(1),
    };
    for entry in [&transferable, &skipped] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(transferable.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(skipped.transient_id.as_str())]
    );

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![transferable.transient_id]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation"),
        vec![skipped]
    );
}

#[test]
fn peer_sync_rejects_malformed_wanted_ids_without_mutating_queue() {
    let daemon = RpcDaemon::test_instance();
    let pending = PropagationEntryRecord {
        transient_id: "a3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-invalid-wanted", pending.transient_id.as_str())
        .expect("mark unhandled");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-invalid-wanted",
                "wanted_ids": ["not-a-transient-id"],
            }),
        ))
        .expect_err("malformed wanted ids should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-invalid-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-invalid-wanted")
            .expect("pending propagation"),
        vec![pending]
    );
}

#[test]
fn peer_sync_rejects_unknown_wanted_ids_without_mutating_queue() {
    let daemon = RpcDaemon::test_instance();
    let pending = PropagationEntryRecord {
        transient_id: "a8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unknown-wanted", pending.transient_id.as_str())
        .expect("mark unhandled");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-unknown-wanted",
                "wanted_ids": ["ff".repeat(32)],
            }),
        ))
        .expect_err("unknown wanted ids should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("current peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-unknown-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-unknown-wanted")
            .expect("pending propagation"),
        vec![pending]
    );
}

#[test]
fn peer_sync_rejects_unknown_wanted_ids_without_creating_new_peer_queue() {
    let daemon = RpcDaemon::test_instance();
    let existing = PropagationEntryRecord {
        transient_id: "aa".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&existing).expect("store propagation entry");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-new-unknown-wanted",
                "wanted_ids": ["ff".repeat(32)],
            }),
        ))
        .expect_err("unknown wanted ids should be rejected before peer queue creation");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("current peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-new-unknown-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-new-unknown-wanted")
            .expect("pending propagation")
            .is_empty(),
        "rejected wanted IDs must not queue existing propagation for a new peer"
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-new-unknown-wanted")),
        "rejected wanted IDs must not create a peer record"
    );
}

#[test]
fn peer_sync_rejects_offer_response_without_existing_peer_queue() {
    let daemon = RpcDaemon::test_instance();
    let existing = PropagationEntryRecord {
        transient_id: "ab".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&existing).expect("store propagation entry");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-new-valid-wanted",
                "wanted_ids": [existing.transient_id.as_str()],
            }),
        ))
        .expect_err("offer response should require an existing peer offer");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("existing peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-new-valid-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-new-valid-wanted")
            .expect("pending propagation")
            .is_empty(),
        "rejected offer response must not queue existing propagation for a new peer"
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-new-valid-wanted")),
        "rejected offer response must not create a peer record"
    );
}
