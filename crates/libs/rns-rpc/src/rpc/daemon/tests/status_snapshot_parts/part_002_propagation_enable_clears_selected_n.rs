#[test]
fn propagation_enable_clears_selected_node_when_static_policy_rejects_it() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            31,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-old"],
            }),
        ))
        .expect("enable old static peer");
    daemon
        .handle_rpc(rpc_request(
            32,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-static-old" }),
        ))
        .expect("select old static peer");

    daemon
        .handle_rpc(rpc_request(
            33,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-new"],
            }),
        ))
        .expect("replace static peer list");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 34,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 35, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 36, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(daemon_status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn propagation_enable_unpeers_removed_static_peers_when_static_only() {
    let daemon = RpcDaemon::test_instance();
    let entry = PropagationEntryRecord {
        transient_id: "af".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_105,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .handle_rpc(rpc_request(
            37,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-old"],
            }),
        ))
        .expect("enable old static peer");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-static-old", entry.transient_id.as_str())
        .expect("mark old peer unhandled");
    daemon
        .handle_rpc(rpc_request(
            38,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-static-old" }),
        ))
        .expect("select old static peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let update = daemon
        .handle_rpc(rpc_request(
            39,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-new"],
            }),
        ))
        .expect("replace static peer list")
        .result
        .expect("replace static peer result");
    assert_eq!(update["propagation"]["selected_node"], JsonValue::Null);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 40, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-static-old")));
    let new_peer = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-new"))
        .expect("new static peer");
    assert_eq!(new_peer["peer_type"].as_str(), Some("static"));
    assert_eq!(new_peer["type"].as_str(), Some("static"));

    let old_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-static-old")
        .expect("old peer pending propagation");
    assert!(old_pending.is_empty());
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("static-only peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-static-old"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["reason"].as_str(), Some("static_only_policy"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 41, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));
}

#[test]
fn propagation_enable_static_only_removed_static_peer_counts_all_queue_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            41,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-all-marks"],
            }),
        ))
        .expect("enable old static peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let handled = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(10),
        received_at: 1_700_000_106,
        size_bytes: 10,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "14".repeat(16),
        payload_hex: "14".repeat(20),
        received_at: 1_700_000_107,
        size_bytes: 20,
        stamp_value: None,
    };
    let received = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(30),
        received_at: 1_700_000_108,
        size_bytes: 30,
        stamp_value: None,
    };
    let transfer_limited = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(40),
        received_at: 1_700_000_109,
        size_bytes: 40,
        stamp_value: None,
    };
    for entry in [&handled, &unhandled, &received, &transfer_limited] {
        daemon.store.upsert_propagation_entry(entry).expect("store entry");
    }
    daemon
        .store
        .mark_peer_handled_propagation(
            "peer-static-all-marks",
            handled.transient_id.as_str(),
        )
        .expect("mark handled");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-static-all-marks",
            unhandled.transient_id.as_str(),
        )
        .expect("mark unhandled");
    daemon
        .store
        .mark_peer_received_propagation(
            "peer-static-all-marks",
            received.transient_id.as_str(),
        )
        .expect("mark received");
    daemon
        .store
        .mark_peer_transfer_limited_propagation(
            "peer-static-all-marks",
            transfer_limited.transient_id.as_str(),
        )
        .expect("mark transfer limited");

    daemon
        .handle_rpc(rpc_request(
            42,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-replacement"],
            }),
        ))
        .expect("replace static peer list");

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("static-only peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-static-all-marks"));
    assert_eq!(event.payload["reason"].as_str(), Some("static_only_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(4));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(100));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[
            json!(handled.transient_id.as_str()),
            json!(received.transient_id.as_str()),
            json!(transfer_limited.transient_id.as_str()),
        ]
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-static-all-marks")
            .expect("remaining handled ids"),
        Vec::<String>::new()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-static-all-marks")
            .expect("remaining unhandled entries"),
        Vec::<PropagationEntryRecord>::new()
    );
}

#[test]
fn propagation_enable_static_only_unpeers_existing_non_static_peers() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            42,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable autopeering");

    let entry = PropagationEntryRecord {
        transient_id: "b0".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_106,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-static-only".to_string(),
            1_700_000_107,
            Some("Auto Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(4)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept autopeer announce");
    daemon
        .handle_rpc(rpc_request(
            43,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto-static-only" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .handle_rpc(rpc_request(
            44,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-allowed"],
            }),
        ))
        .expect("enable static-only policy");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 45, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-static-only")));
    assert!(rows.iter().any(|row| {
        row["peer"].as_str() == Some("peer-static-allowed")
            && row["peer_type"].as_str() == Some("static")
    }));

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-static-only")
            .expect("autopeer marks after static-only")
            .is_empty(),
        "static-only policy should clear non-static peer queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("static-only non-static peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-static-only"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("static_only_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 46,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}
