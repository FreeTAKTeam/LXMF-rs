#[test]
fn peer_sync_reports_python_status_type_alias() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            55,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-alias"],
            }),
        ))
        .expect("enable propagation");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let static_result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-static-alias" })))
        .expect("static peer sync")
        .result
        .expect("static peer sync result");
    assert_eq!(static_result["peer_type"].as_str(), Some("static"));
    assert_eq!(static_result["type"].as_str(), Some("static"));

    let static_event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("static peer sync event");
    assert_eq!(static_event.payload["peer_type"].as_str(), Some("static"));
    assert_eq!(static_event.payload["type"].as_str(), Some("static"));
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let manual_result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": "peer-manual-alias" })))
        .expect("manual peer sync")
        .result
        .expect("manual peer sync result");
    assert_eq!(manual_result["peer_type"].as_str(), Some("manual"));
    assert_eq!(manual_result["type"].as_str(), Some("discovered"));

    let manual_event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("manual peer sync event");
    assert_eq!(manual_event.payload["peer_type"].as_str(), Some("manual"));
    assert_eq!(manual_event.payload["type"].as_str(), Some("discovered"));
}

#[test]
fn stale_high_cost_announce_does_not_remove_newer_autopeer() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            55,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_400,
            None,
            None,
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
        .expect("accept initial announce");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_399,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(9)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept stale high-cost announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-auto"));
    assert_eq!(row["peer_type"].as_str(), Some("auto"));
}

#[test]
fn high_cost_announce_breaks_existing_manual_peer_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            57,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    daemon
        .handle_rpc(rpc_request(58, "peer_sync", json!({ "peer": "peer-manual" })))
        .expect("manual peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_499,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-manual", entry.transient_id.as_str())
        .expect("mark manual peer propagation unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .accept_announce_with_metadata(
            "peer-manual".to_string(),
            1_700_000_500,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(9)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept high-cost announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 59, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-manual")
            .expect("manual peer propagation marks after break")
            .is_empty(),
        "breaking a manual peer should clear stale propagation queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("manual peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-manual"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("peering_cost_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
}

#[test]
fn high_cost_announce_breaks_existing_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            60,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    let stored_peer = "Peer-Manual-High-Cost-Case";
    let announce_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": stored_peer })))
        .expect("manual peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "f4".repeat(32),
        destination: "14".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_501,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer, entry.transient_id.as_str())
        .expect("mark manual peer propagation unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .accept_announce_with_metadata(
            announce_peer,
            1_700_000_502,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(9)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept high-cost announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 62, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("manual peer propagation marks after break")
            .is_empty()
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("manual peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some(stored_peer));
    assert_eq!(event.payload["reason"].as_str(), Some("peering_cost_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
}
