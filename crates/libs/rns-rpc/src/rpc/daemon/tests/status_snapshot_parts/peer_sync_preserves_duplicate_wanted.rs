#[test]
fn peer_sync_preserves_duplicate_wanted_ids_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xae);
    let wanted = PropagationEntryRecord {
        transient_id: "af".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_608,
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
            56,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str(), wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let expected_resource_bytes = rmp_serde::to_vec(&(1.0_f64, vec![
        vec![0x14; 24],
        vec![0x14; 24],
    ]))
    .expect("pack duplicate wanted resource")
    .len();

    assert_eq!(result["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(48));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(2));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(2.0));
    assert_eq!(result["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(wanted.transient_id.as_str()), json!(wanted.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["messages"].as_array().expect("transferred messages").len(),
        2
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
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(2));
    assert_eq!(event.payload["acceptance_rate"].as_f64(), Some(2.0));
    assert_eq!(
        event.payload["propagation"]["transferred_ids"]
            .as_array()
            .expect("event transferred ids"),
        &[json!(wanted.transient_id.as_str()), json!(wanted.transient_id.as_str())]
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 57, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(2));
    assert_eq!(row["offered"].as_u64(), Some(1));
    assert_eq!(row["outgoing"].as_u64(), Some(2));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(2.0));
}

#[test]
fn peer_sync_boolean_wanted_ids_true_transfers_all_offered_messages_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([3u8; 16]);
    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_606,
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
        .expect("accept ready propagation peer announce");
    let first = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(30),
        received_at: 1_700_000_608,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&first, &second] {
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
                "peer": peer,
                "wanted_ids": true,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(2));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(first.transient_id.as_str()), json!(second.transient_id.as_str())]
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
fn peer_sync_boolean_wanted_ids_true_keeps_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-wants-all-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-wants-all-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "b7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "17".repeat(24),
        received_at: 1_700_000_609,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-wants-all-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-wants-all-policy",
                "wanted_ids": true,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-wants-all-policy")
        .expect("pending propagation");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_selected_wanted_ids_keep_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-selected-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-selected-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "b8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "18".repeat(24),
        received_at: 1_700_000_610,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-selected-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-selected-policy",
                "wanted_ids": [entry.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-selected-policy")
        .expect("pending propagation");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_empty_wanted_ids_keep_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-wants-none-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-wants-none-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "b9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "19".repeat(24),
        received_at: 1_700_000_611,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-wants-none-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-wants-none-policy",
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-wants-none-policy")
        .expect("pending propagation");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_boolean_wanted_ids_false_handles_all_offered_messages_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb5);
    let already_known = PropagationEntryRecord {
        transient_id: "b5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
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
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![already_known.transient_id]
    );
}
