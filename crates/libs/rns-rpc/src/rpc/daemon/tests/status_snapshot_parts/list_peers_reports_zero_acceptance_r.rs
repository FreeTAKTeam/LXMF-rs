#[test]
fn list_peers_reports_zero_acceptance_rate_when_no_offers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-derived-rate" })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-derived-rate").expect("peer record");
        peer.acceptance_rate = 0.9;
    }

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-derived-rate"));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
}

#[test]
fn list_peers_preserves_python_peer_message_counters() {
    let daemon = RpcDaemon::test_instance();
    let peer: PeerRecord = serde_json::from_value(json!({
        "peer": "peer-python-counters",
        "last_seen": 1_700_001_100,
        "offered": 7,
        "outgoing": 5,
        "incoming": 3,
        "acceptance_rate": 0.9,
    }))
    .expect("deserialize python counter peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.peer.clone(), peer);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-python-counters"))
        .expect("peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(7));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(5));
    assert_eq!(row["messages"]["incoming"].as_u64(), Some(3));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(5.0 / 7.0));
}

#[test]
fn announce_received_returns_enriched_peer_accounting_like_list_peers() {
    let daemon = RpcDaemon::test_instance();
    let peer: PeerRecord = serde_json::from_value(json!({
        "peer": "peer-announce-accounting",
        "last_seen": 1_700_001_100,
        "offered": 7,
        "outgoing": 5,
        "incoming": 3,
        "acceptance_rate": 0.9,
    }))
    .expect("deserialize python counter peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.peer.clone(), peer);

    let response = daemon
        .handle_rpc(rpc_request(
            55,
            "announce_received",
            json!({
                "peer": "peer-announce-accounting",
                "timestamp": 1_700_001_200,
                "name": "Announced Accounting Peer",
                "name_source": "test",
            }),
        ))
        .expect("announce received")
        .result
        .expect("announce result");
    let peer = &response["peer"];

    assert_eq!(peer["peer"].as_str(), Some("peer-announce-accounting"));
    assert_eq!(peer["messages"]["offered"].as_u64(), Some(7));
    assert_eq!(peer["messages"]["outgoing"].as_u64(), Some(5));
    assert_eq!(peer["messages"]["incoming"].as_u64(), Some(3));
    assert_eq!(peer["offered"].as_u64(), Some(7));
    assert_eq!(peer["outgoing"].as_u64(), Some(5));
    assert_eq!(peer["incoming"].as_u64(), Some(3));
    assert_eq!(peer["acceptance_rate"].as_f64(), Some(5.0 / 7.0));
}

#[test]
fn peer_sync_without_offers_preserves_failure_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-backoff-no-offers" })))
        .expect("initial peer sync");
    daemon.record_outbound_peer_activity("peer-backoff-no-offers", 64, false);

    let before = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let before_row = before["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-backoff-no-offers"))
        .expect("peer row");
    let sync_backoff = before_row["sync_backoff"].as_u64().expect("sync backoff");
    let next_sync_attempt =
        before_row["next_sync_attempt"].as_i64().expect("next sync attempt");
    assert!(sync_backoff > 0);
    assert!(next_sync_attempt > 0);

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-backoff-no-offers" })))
        .expect("no-offer peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));

    let after = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let after_row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-backoff-no-offers"))
        .expect("peer row");
    assert_eq!(after_row["sync_backoff"].as_u64(), Some(sync_backoff));
    assert_eq!(after_row["next_sync_attempt"].as_i64(), Some(next_sync_attempt));
    assert_eq!(after_row["alive"].as_bool(), Some(false));
}

#[test]
fn peer_sync_without_offers_preserves_liveness_when_not_backing_off_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-empty-alive" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-empty-alive").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-empty-alive" })))
        .expect("empty peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.5));
    let last_sync_attempt = result["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-empty-alive"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("empty peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-empty-alive"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
}

#[test]
fn peer_sync_during_backoff_postpones_skipped_offers() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x47);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
        record.peering_timebase = 1_700_000_000;
        record.network_distance = 3;
    }
    let previous_transfer = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "19".repeat(40),
        received_at: 1_700_000_613,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&previous_transfer).expect("store previous transfer");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            previous_transfer.transient_id.as_str(),
        )
        .expect("mark previous transfer unhandled");
    daemon
        .handle_rpc(rpc_request(
            53,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with previous transfer");
    let previous_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x19; 40]])).expect("pack sync resource").len();
    daemon.record_outbound_peer_activity(peer.as_str(), 64, false);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(24);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_614,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let before = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let before_row = before["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    let sync_backoff = before_row["sync_backoff"].as_u64().expect("sync backoff");
    let next_sync_attempt =
        before_row["next_sync_attempt"].as_i64().expect("next sync attempt");
    assert!(sync_backoff > 0);
    assert!(next_sync_attempt > 0);

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("skipped peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["state"].as_u64(), Some(0));
    assert_eq!(result["state_name"].as_str(), Some("idle"));
    assert_eq!(result["sync_schedule_state"].as_str(), Some("backoff"));
    assert_eq!(result["sync_schedule_reason"].as_str(), Some("backoff"));
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["ler"].as_u64(), Some(0));
    assert_eq!(result["network_distance"].as_u64(), Some(3));
    assert_eq!(result["peering_timebase"].as_i64(), Some(1_700_000_000));
    assert_eq!(result["rx_bytes"].as_u64(), Some(0));
    assert_eq!(result["tx_bytes"].as_u64(), Some((previous_resource_bytes + 64) as u64));
    assert_eq!(result["alive"].as_bool(), Some(false));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(previous_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(previous_resource_bytes as u64));
    assert!(result["last_heard"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(result["propagation"]["synced"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer limited ids"),
        &[] as &[JsonValue]
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
    assert_eq!(event.payload["postponed"].as_bool(), Some(true));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));

    let after = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let after_row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(after_row["sync_backoff"].as_u64(), Some(sync_backoff));
    assert_eq!(after_row["next_sync_attempt"].as_i64(), Some(next_sync_attempt));
    assert_eq!(after_row["sync_transfer_rate"].as_f64(), Some(previous_resource_bytes as f64));
    assert_eq!(after_row["str"].as_u64(), Some(previous_resource_bytes as u64));
}

#[test]
fn forced_peer_sync_bypasses_existing_backoff_like_manual_control() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x48);
    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_614,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon.record_outbound_peer_activity(peer.as_str(), 64, false);

    let postponed = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("default peer sync")
        .result
        .expect("default peer sync result");
    assert_eq!(postponed["synced"].as_bool(), Some(false));
    assert_eq!(postponed["postpone_reason"].as_str(), Some("backoff"));

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "force_sync": true,
            }),
        ))
        .expect("forced peer sync")
        .result
        .expect("forced peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_ne!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred_ids"], json!([entry.transient_id]));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
}
