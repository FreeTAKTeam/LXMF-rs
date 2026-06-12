#[test]
fn peer_activation_snapshots_case_insensitive_preexisting_completed_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Late-Completed-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    let entry = PropagationEntryRecord {
        transient_id: "e5".repeat(32),
        destination: "29".repeat(16),
        payload_hex: "29".repeat(24),
        received_at: 1_700_000_953,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .record_peer_transferred_propagation(request_peer.as_str(), entry.transient_id.as_str())
        .expect("record transfer before peer activation");

    daemon.record_propagation_offer_peer(stored_peer).expect("activate propagation peer");

    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(stored_peer).expect("handled ids"),
        vec![entry.transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("unhandled propagation")
            .is_empty()
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("peer record");
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
fn peer_unpeer_clears_persisted_propagation_queue_marks() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x52);
    daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("sync peer");
    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(24),
        received_at: 1_700_000_920,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), "fa".repeat(32).as_str())
        .expect("mark stale unhandled");

    let unpeer = daemon
        .handle_rpc(rpc_request(91, "peer_unpeer", json!({ "peer": peer.as_str() })))
        .expect("unpeer peer")
        .result
        .expect("unpeer result");
    assert_eq!(unpeer["removed"].as_bool(), Some(true));
    assert_eq!(unpeer["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(unpeer["propagation_cleared_bytes"].as_u64(), Some(24));
    assert_eq!(unpeer["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(unpeer["messages"]["unhandled"].as_u64(), Some(1));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("list unhandled")
            .is_empty()
    );

    make_ready_propagation_peer(&daemon, 0x52);
    daemon
        .handle_rpc(rpc_request(
            92,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("resync peer");
    let peers = daemon
        .handle_rpc(RpcRequest { id: 93, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn peer_unpeer_clears_case_variant_completed_queue_marks_before_reactivation_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Unpeer-Case-Completed";
    let request_peer = stored_peer.to_ascii_lowercase();
    let entry = PropagationEntryRecord {
        transient_id: "e6".repeat(32),
        destination: "30".repeat(16),
        payload_hex: "30".repeat(24),
        received_at: 1_700_000_954,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .record_peer_transferred_propagation(request_peer.as_str(), entry.transient_id.as_str())
        .expect("record transfer before peer activation");
    daemon.record_propagation_offer_peer(stored_peer).expect("activate propagation peer");

    daemon
        .handle_rpc(rpc_request(94, "peer_unpeer", json!({ "peer": stored_peer })))
        .expect("unpeer peer");
    daemon.record_propagation_offer_peer(stored_peer).expect("reactivate propagation peer");

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids(stored_peer)
            .expect("handled ids after reactivation")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("unhandled propagation after reactivation")
            .into_iter()
            .map(|entry| entry.transient_id)
            .collect::<Vec<_>>(),
        vec![entry.transient_id.clone()]
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("peer record");
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

#[test]
fn clear_peers_clears_persisted_propagation_queue_marks() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-clear-queue" })))
        .expect("sync peer");
    let entry = PropagationEntryRecord {
        transient_id: "ca".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(12),
        received_at: 1_700_000_706,
        size_bytes: 12,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-clear-queue", entry.transient_id.as_str())
        .expect("mark unhandled");

    daemon
        .handle_rpc(RpcRequest { id: 91, method: "clear_peers".to_string(), params: None })
        .expect("clear peers");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-clear-queue")
            .expect("list unhandled")
            .is_empty()
    );
}

#[test]
fn clear_peers_clears_selected_propagation_node() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            92,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-clear" }),
        ))
        .expect("select propagation node");

    daemon
        .handle_rpc(RpcRequest { id: 93, method: "clear_peers".to_string(), params: None })
        .expect("clear peers");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 95, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 96, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(daemon_status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn clear_all_clears_selected_propagation_node() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            92,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-clear-all" }),
        ))
        .expect("select propagation node");

    daemon
        .handle_rpc(RpcRequest { id: 93, method: "clear_all".to_string(), params: None })
        .expect("clear all");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 95, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn peer_unpeer_clears_selected_propagation_node() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            92,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-unpeer" }),
        ))
        .expect("select propagation node");

    daemon
        .handle_rpc(rpc_request(93, "peer_unpeer", json!({ "peer": "peer-selected-unpeer" })))
        .expect("unpeer selected node");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 95, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 96, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(daemon_status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn peer_unpeer_matches_existing_peer_case_insensitively_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let stored_peer = "Cd".repeat(16);
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .accept_announce_with_metadata(
            stored_peer.clone(),
            1_700_000_940,
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
    daemon
        .handle_rpc(rpc_request(
            96,
            "set_outbound_propagation_node",
            json!({ "peer": stored_peer.as_str() }),
        ))
        .expect("select mixed-case peer");
    let entry = PropagationEntryRecord {
        transient_id: "d2".repeat(32),
        destination: "2d".repeat(16),
        payload_hex: "55".repeat(24),
        received_at: 1_700_000_941,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer.as_str(), entry.transient_id.as_str())
        .expect("mark mixed-case peer unhandled");

    let result = daemon
        .handle_rpc(rpc_request(97, "peer_unpeer", json!({ "peer": request_peer })))
        .expect("unpeer with lower-case id")
        .result
        .expect("unpeer result");

    assert_eq!(result["peer"].as_str(), Some(stored_peer.as_str()));
    assert_eq!(result["removed"].as_bool(), Some(true));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(1));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer.as_str())
            .expect("mixed-case unhandled")
            .is_empty()
    );

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 98,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 99, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"].as_array().expect("peer rows").is_empty(),
        "unpeered mixed-case peer should not remain active"
    );
}
