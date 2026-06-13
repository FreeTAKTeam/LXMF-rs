#[test]
fn discovered_announce_bursts_do_not_collapse_in_announce_log() {
    let daemon = RpcDaemon::test_instance();
    let timestamp = 1_700_000_250;

    for idx in 0..4 {
        daemon
            .accept_announce_with_metadata(
                "peer-discovered".to_string(),
                timestamp,
                Some(format!("Peer Discovered {idx}")),
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
            .expect("accept discovered announce");
    }

    let announces = daemon
        .handle_rpc(RpcRequest { id: 50, method: "list_announces".to_string(), params: None })
        .expect("list announces")
        .result
        .expect("list announces result");
    let rows = announces["announces"].as_array().expect("announce rows");
    let matching = rows
        .iter()
        .filter(|row| row["peer"].as_str() == Some("peer-discovered"))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 4, "same-second discovered announces must remain distinct");
    let unique_ids = matching
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_ids.len(), 4, "announce log IDs must be unique for burst traffic");

    let legacy_event_ids = std::iter::from_fn(|| daemon.take_event())
        .filter(|event| event.event_type == "announce_received")
        .filter_map(|event| event.payload["id"].as_str().map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        legacy_event_ids.len(),
        4,
        "daemon event queue must expose unique announce IDs for burst traffic"
    );

    let events = daemon
        .handle_rpc(rpc_request(51, "sdk_poll_events_v2", json!({ "cursor": null, "max": 20 })))
        .expect("poll sdk events")
        .result
        .expect("sdk events result");
    let event_rows = events["events"].as_array().expect("event rows");
    let announce_event_ids = event_rows
        .iter()
        .filter(|row| row["event_type"].as_str() == Some("announce_received"))
        .filter_map(|row| row["payload"]["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        announce_event_ids.len(),
        4,
        "SDK announce events must expose unique IDs for burst traffic"
    );
}

#[test]
fn peering_cost_policy_blocks_and_breaks_autopeers() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            50,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_299,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_300,
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
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto")
            .expect("queued autopeer propagation"),
        vec![entry.clone()]
    );
    daemon
        .handle_rpc(rpc_request(
            51,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_301,
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
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto")
            .expect("autopeer propagation marks after break")
            .is_empty(),
        "breaking an autopeer should clear stale propagation queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("autopeer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("peering_cost_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 53,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}

#[test]
fn peer_activity_updates_runtime_counters() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            50,
            "peer_sync",
            json!({
                "peer": "peer-runtime",
            }),
        ))
        .expect("peer sync");

    daemon.record_inbound_peer_activity("peer-runtime", 120);
    daemon.record_outbound_peer_activity("peer-runtime", 80, true);
    daemon.record_outbound_peer_activity("peer-runtime", 40, false);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-runtime"));
    assert_eq!(row["rx_bytes"].as_u64(), Some(120));
    assert_eq!(row["tx_bytes"].as_u64(), Some(120));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(
        row["next_sync_attempt"].as_i64(),
        Some(row["last_sync_attempt"].as_i64().expect("last sync attempt") + 12 * 60)
    );
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 1.0));
}

#[test]
fn successful_remote_peer_activity_keeps_newer_failure_backoff() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-activity-order";
    daemon
        .handle_rpc(rpc_request(51, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.alive = false;
        record.sync_backoff = 12 * 60;
        record.next_sync_attempt = now_i64().saturating_add(12 * 60);
    }

    assert!(daemon.record_successful_remote_propagation_peer_activity_count(peer, 120, 2));
    daemon.record_outbound_peer_activity(peer, 40, false);

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("peer record");
    assert_eq!(record.incoming, 2);
    assert_eq!(record.rx_bytes, 120);
    assert_eq!(record.tx_bytes, 40);
    assert!(!record.alive);
    assert_eq!(record.sync_backoff, 12 * 60);
    assert_eq!(record.next_sync_attempt, record.last_sync_attempt.saturating_add(12 * 60));
}

#[test]
fn inbound_peer_activity_matches_existing_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Inbound-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(51, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");

    daemon.record_inbound_peer_activity(request_peer.as_str(), 120);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    let row = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some(stored_peer))
        .expect("stored peer row");
    assert_eq!(row["rx_bytes"].as_u64(), Some(120));
    assert_eq!(row["alive"].as_bool(), Some(true));
    let last_seen = row["last_seen"].as_i64().expect("last_seen");
    assert!(last_seen > 0);
    assert_eq!(row["last_heard"].as_i64(), Some(last_seen));
}

#[test]
fn delivered_peer_activity_updates_last_heard_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(51, "peer_sync", json!({ "peer": "peer-delivered-heard" })))
        .expect("peer sync");

    daemon.record_outbound_peer_activity("peer-delivered-heard", 64, true);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    let last_seen = row["last_seen"].as_i64().expect("last_seen");
    assert!(last_seen > 0);
    assert_eq!(row["last_heard"].as_i64(), Some(last_seen));
    assert_eq!(row["last_sync_attempt"].as_i64(), Some(last_seen));
}

#[test]
fn sent_peer_activity_does_not_mark_peer_heard_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            51,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-sent-only"],
            }),
        ))
        .expect("enable static peer");

    daemon.record_outbound_peer_sent("peer-sent-only", 64);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-sent-only"));
    assert_eq!(row["tx_bytes"].as_u64(), Some(64));
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["last_heard"].as_i64(), Some(0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
    assert!(row["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
}

#[test]
fn outbound_peer_activity_matches_existing_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Outbound-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");

    daemon.record_outbound_peer_sent(request_peer.as_str(), 64);
    daemon.record_outbound_peer_activity(request_peer.as_str(), 32, true);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    let row = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some(stored_peer))
        .expect("stored peer row");
    assert_eq!(row["tx_bytes"].as_u64(), Some(96));
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    let last_seen = row["last_seen"].as_i64().expect("last_seen");
    assert!(last_seen > 0);
    assert_eq!(row["last_heard"].as_i64(), Some(last_seen));
    assert_eq!(row["last_sync_attempt"].as_i64(), Some(last_seen));
}

#[test]
fn failed_peer_activity_does_not_mark_unheard_static_peer_alive() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            52,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-failed"],
            }),
        ))
        .expect("enable static peer");

    daemon.record_outbound_peer_activity("peer-static-failed", 32, false);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-static-failed"));
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["last_heard"].as_i64(), Some(0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
}

#[test]
fn new_peer_acceptance_rate_matches_python_zero_offer_default() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-zero-offers" })))
        .expect("peer sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-zero-offers"));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
}
