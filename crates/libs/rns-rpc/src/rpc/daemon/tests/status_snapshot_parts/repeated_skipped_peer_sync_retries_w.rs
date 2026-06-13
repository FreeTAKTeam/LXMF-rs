#[test]
fn repeated_skipped_peer_sync_retries_without_failure_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-skipped-repeat" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-skipped-repeat").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
        peer.peering_key_value = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ea".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "1a".repeat(20),
        received_at: 1_700_000_616,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-skipped-repeat", entry.transient_id.as_str())
        .expect("mark unhandled");

    daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-skipped-repeat" })))
        .expect("first skipped peer sync");
    let first = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let first_row = first["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-skipped-repeat"))
        .expect("peer row");
    let first_attempt = first_row["last_sync_attempt"].as_i64().expect("first attempt");
    assert_eq!(first_row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(first_row["next_sync_attempt"].as_i64(), Some(0));

    let second_result = daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-skipped-repeat" })))
        .expect("second skipped peer sync")
        .result
        .expect("second peer sync result");
    assert_eq!(second_result["synced"].as_bool(), Some(true));
    assert_eq!(second_result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(second_result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(second_result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(second_result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(second_result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(second_result["next_sync_attempt"].as_i64(), Some(0));

    let second = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let second_row = second["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-skipped-repeat"))
        .expect("peer row");
    assert_eq!(second_row["sync_backoff"].as_u64(), Some(0));
    assert!(second_row["last_sync_attempt"].as_i64().is_some_and(|value| value >= first_attempt));
    assert_eq!(second_row["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_with_only_skipped_offers_clears_failure_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-skipped-initial" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-skipped-initial").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
        peer.peering_key_value = Some(1);
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = 0;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(20),
        received_at: 1_700_000_615,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-skipped-initial", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-skipped-initial" })))
        .expect("skipped peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-skipped-initial"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_result_and_event_report_skipped_only_completion() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-backoff-report" })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-backoff-report").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
        peer.peering_key_value = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ba".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(20),
        received_at: 1_700_000_618,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-backoff-report", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-backoff-report" })))
        .expect("skipped peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    let last_sync_attempt = result["last_sync_attempt"].as_i64().expect("last sync attempt");
    let last_heard = result["last_heard"].as_i64().expect("last heard");
    assert!(last_sync_attempt > 0);
    assert!(last_heard > 0);
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(event.payload["last_heard"].as_i64(), Some(last_heard));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(event.payload["propagation"]["skipped"].as_u64(), Some(1));
}

#[test]
fn list_peers_exposes_python_style_message_counters() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-message-stats" })))
        .expect("peer sync");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-message-stats-in".to_string(),
            source: "peer-message-stats".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_600,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("accept inbound");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-message-stats-out".to_string(),
            source: "local".to_string(),
            destination: "peer-message-stats".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_601,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-message-stats"))
        .expect("peer row");
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
}

#[test]
fn peer_sync_marks_unhandled_propagation_entries_handled() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x41);
    let entry = PropagationEntryRecord {
        transient_id: "ab".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(16),
        received_at: 1_700_000_605,
        size_bytes: 16,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark propagation unhandled");

    daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("peer sync");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("list handled"),
        vec![entry.transient_id]
    );
}

#[test]
fn peer_sync_queues_existing_entries_for_new_manual_peer() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([0x42_u8; 16]);
    daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("seed manual peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_stamp_cost = Some(1);
        record.propagation_stamp_cost_flexibility = Some(1);
        record.peering_cost = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ac".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_606,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("manual"));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_offer_response_only_transfers_wanted_messages_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xad);
    let wanted = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "ae".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(30),
        received_at: 1_700_000_608,
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

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["offered"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["offered"].as_u64(), Some(2));
    assert_eq!(result["outgoing"].as_u64(), Some(1));
    assert_eq!(result["incoming"].as_u64(), Some(0));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.5));
    assert_eq!(
        result["propagation"]["messages"].as_array().expect("transferred messages").len(),
        1
    );
    assert_eq!(
        result["propagation"]["messages"][0]["transient_id"].as_str(),
        Some(wanted.transient_id.as_str())
    );

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![wanted.transient_id, already_known.transient_id]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
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
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["incoming"].as_u64(), Some(0));
}
