#[test]
fn peer_sync_result_and_event_report_transfer_and_stamp_policy() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(65, "peer_sync", json!({ "peer": "peer-sync-policy" })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-sync-policy").expect("peer record");
        peer.name = Some("Policy Peer".to_string());
        peer.name_source = Some("test".to_string());
        peer.peer_type = Some("static".to_string());
        peer.first_seen = 1_700_000_111;
        peer.seen_count = 3;
        peer.propagation_transfer_limit = Some(333);
        peer.propagation_sync_limit = Some(999);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.sync_transfer_rate = 12_345.0;
        peer.peering_timebase = 1_700_000_123;
        peer.network_distance = 4;
        peer.rx_bytes = 55;
        peer.tx_bytes = 77;
    }

    let result = daemon
        .handle_rpc(rpc_request(66, "peer_sync", json!({ "peer": "peer-sync-policy" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("static"));
    assert_eq!(result["name"].as_str(), Some("Policy Peer"));
    assert_eq!(result["name_source"].as_str(), Some("test"));
    assert_eq!(result["first_seen"].as_i64(), Some(1_700_000_111));
    assert_eq!(result["seen_count"].as_u64(), Some(4));
    assert_eq!(result["state"].as_u64(), Some(0));
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["ler"].as_u64(), Some(0));
    assert_eq!(result["network_distance"].as_u64(), Some(4));
    assert_eq!(result["peering_timebase"].as_i64(), Some(1_700_000_123));
    assert_eq!(result["rx_bytes"].as_u64(), Some(55));
    assert_eq!(result["tx_bytes"].as_u64(), Some(77));
    assert_eq!(result["propagation_transfer_limit"].as_u64(), Some(333));
    assert_eq!(result["propagation_sync_limit"].as_u64(), Some(999));
    assert_eq!(result["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(result["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(result["transfer_limit"].as_u64(), Some(333));
    assert_eq!(result["sync_limit"].as_u64(), Some(999));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(333));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(999));
    assert_eq!(result["propagation"]["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(result["propagation"]["stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(result["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(result["stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(12_345.0));
    assert_eq!(result["str"].as_u64(), Some(12_345));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["peer_type"].as_str(), Some("static"));
    assert_eq!(event.payload["name"].as_str(), Some("Policy Peer"));
    assert_eq!(event.payload["name_source"].as_str(), Some("test"));
    assert_eq!(event.payload["first_seen"].as_i64(), Some(1_700_000_111));
    assert_eq!(event.payload["seen_count"].as_u64(), Some(4));
    assert_eq!(event.payload["state"].as_u64(), Some(0));
    assert_eq!(event.payload["sync_strategy"].as_u64(), Some(2));
    assert_eq!(event.payload["ler"].as_u64(), Some(0));
    assert_eq!(event.payload["network_distance"].as_u64(), Some(4));
    assert_eq!(event.payload["peering_timebase"].as_i64(), Some(1_700_000_123));
    assert_eq!(event.payload["rx_bytes"].as_u64(), Some(55));
    assert_eq!(event.payload["tx_bytes"].as_u64(), Some(77));
    assert_eq!(
        event.payload["propagation_transfer_limit"].as_u64(),
        Some(333)
    );
    assert_eq!(event.payload["propagation_sync_limit"].as_u64(), Some(999));
    assert_eq!(event.payload["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(
        event.payload["propagation_stamp_cost_flexibility"].as_u64(),
        Some(2)
    );
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(333));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(999));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(333));
    assert_eq!(event.payload["propagation"]["sync_limit"].as_u64(), Some(999));
    assert_eq!(event.payload["propagation"]["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(
        event.payload["propagation"]["stamp_cost_flexibility"].as_u64(),
        Some(2)
    );
    assert_eq!(event.payload["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(event.payload["stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(event.payload["sync_transfer_rate"].as_f64(), Some(12_345.0));
    assert_eq!(event.payload["str"].as_u64(), Some(12_345));
}

#[test]
fn list_peers_includes_propagation_marks_in_message_counters() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-propagation-stats" })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-propagation-stats").expect("peer record");
        peer.acceptance_rate = 0.9;
    }
    let handled = PropagationEntryRecord {
        transient_id: "ac".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(16),
        received_at: 1_700_000_606,
        size_bytes: 16,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "14".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon
        .store
        .mark_peer_handled_propagation("peer-propagation-stats", handled.transient_id.as_str())
        .expect("mark handled");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-propagation-stats", unhandled.transient_id.as_str())
        .expect("mark unhandled");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 57, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-propagation-stats"))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(row["messages"]["offered_bytes"].as_u64(), Some(16));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(
        row["handled_ids"].as_array().expect("handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
}

#[test]
fn list_peers_exposes_peering_key_value_when_cost_is_known() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([3u8; 16]);

    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_610,
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
        .expect("accept propagation peer announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["peering_cost"].as_u64(), Some(1));
    assert!(row["peering_key"].as_u64().is_some_and(|value| value >= 1));
}

#[test]
fn list_peers_exposes_peering_key_status_values() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let ready_peer = hex::encode([3u8; 16]);

    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-unconfigured-key" })))
        .expect("create unconfigured peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unconfigured-key").expect("peer record");
        peer.peering_cost = None;
    }
    daemon
        .accept_announce_with_metadata(
            ready_peer.clone(),
            1_700_000_610,
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
    daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-not-ready-key" })))
        .expect("create not-ready peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-not-ready-key").expect("peer record");
        peer.peering_cost = Some(1);
    }

    let peers = daemon
        .handle_rpc(RpcRequest { id: 57, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    let unconfigured = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-unconfigured-key"))
        .expect("unconfigured peer row");
    let ready = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some(ready_peer.as_str()))
        .expect("ready peer row");
    let not_ready = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-not-ready-key"))
        .expect("not-ready peer row");

    assert_eq!(unconfigured["peering_key"], JsonValue::Null);
    assert_eq!(unconfigured["peering_key_status"].as_str(), Some("unconfigured"));
    assert!(ready["peering_key"].as_u64().is_some_and(|value| value >= 1));
    assert_eq!(ready["peering_key_status"].as_str(), Some("ready"));
    assert_eq!(not_ready["peering_key"], JsonValue::Null);
    assert_eq!(not_ready["peering_key_status"].as_str(), Some("not_ready"));
}

#[test]
fn peer_sync_result_and_event_expose_peering_key_value_when_cost_is_known() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([3u8; 16]);

    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_611,
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
        .expect("accept propagation peer announce");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let peering_key = result["peering_key"].as_u64().expect("peering key");
    assert!(peering_key >= 1);
    assert_eq!(result["propagation"]["peering_key"].as_u64(), Some(peering_key));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["peering_key"].as_u64(), Some(peering_key));
    assert_eq!(event.payload["propagation"]["peering_key"].as_u64(), Some(peering_key));
}

#[test]
fn peer_sync_preserves_existing_auto_peer_type() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            52,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_220,
            Some("Peer Auto".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(4),
            Some(Some(1)),
            Some(Some(4)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept announce");

    daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-auto" })))
        .expect("peer sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer_type"].as_str(), Some("auto"));
}
