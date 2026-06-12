#[test]
fn peer_sync_no_transfer_offer_response_preserves_tx_bytes_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb6);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
        record.tx_bytes = 77;
        record.sync_transfer_rate = 12_345.0;
    }
    let already_known = PropagationEntryRecord {
        transient_id: "b6".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_608,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), already_known.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": false,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(result["tx_bytes"].as_u64(), Some(77));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(12_345.0));

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
    assert_eq!(row["tx_bytes"].as_u64(), Some(77));
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(12_345.0));
}

#[test]
fn peer_sync_matches_wanted_ids_by_canonical_transient_id() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa1);
    let wanted = PropagationEntryRecord {
        transient_id: "a1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&wanted).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), wanted.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [format!("  {}  ", wanted.transient_id.to_ascii_uppercase())],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(wanted.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
}

#[test]
fn peer_sync_handles_unwanted_stamped_entries_without_transfer() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa2);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_608,
        size_bytes: 24,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), already_known.transient_id.as_str())
        .expect("mark unhandled");

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

    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["messages"].as_array().expect("messages").len(), 0);
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(already_known.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![already_known.transient_id]
    );
}

#[test]
fn peer_sync_unwanted_offer_response_does_not_revive_unheard_peer_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa9);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = false;
        record.last_seen = 0;
        record.sync_backoff = 0;
        record.next_sync_attempt = 0;
        record.acceptance_rate = 0.75;
        record.sync_transfer_rate = 12_345.0;
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            already_known.transient_id.as_str(),
        )
        .expect("mark unhandled");

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
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(result["alive"].as_bool(), Some(false));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(12_345.0));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let row = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result")["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .cloned()
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(12_345.0));
}

#[test]
fn peer_sync_transfer_limits_unwanted_oversized_entries_before_offer_response() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa4);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = Some(80);
        record.propagation_sync_limit = Some(1_000);
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(100),
        received_at: 1_700_000_609,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), already_known.transient_id.as_str())
        .expect("mark unhandled");

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

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer-limited ids"),
        &[json!(already_known.transient_id.as_str())]
    );
    assert!(
        result["propagation"]["handled_ids"]
            .as_array()
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![already_known.transient_id]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_keeps_unwanted_sync_limited_entries_queued() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-unwanted-sync-limit" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unwanted-sync-limit").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
        peer.peering_key_value = Some(1);
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(100),
        received_at: 1_700_000_610,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unwanted-sync-limit", already_known.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-unwanted-sync-limit",
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["remaining_bytes"].as_u64(), Some(100));
    assert!(
        result["propagation"]["handled_ids"]
            .as_array()
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(already_known.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-unwanted-sync-limit")
            .expect("handled ids")
            .is_empty()
    );
    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-unwanted-sync-limit")
        .expect("pending propagation");
    assert_eq!(pending, vec![already_known]);
}
