#[test]
fn postponed_propagation_remote_fetch_updates_source_peer_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 0,
            "fetched_count": 0,
            "messages": [],
            "postponed": true,
            "postpone_reason": "throttled",
            "synced": false,
            "error": "remote fetch postponed",
        })),
    }));
    let peer = "peer-remote-fetch-postponed-backoff";
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.alive = true;
        record.sync_backoff = 0;
        record.next_sync_attempt = 0;
        record.acceptance_rate = 0.5;
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "c8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_627,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(81, "propagation_remote_fetch", json!({ "remote": peer })))
        .expect("postponed remote fetch should return the bridge envelope")
        .result
        .expect("remote fetch result");
    assert_eq!(result["result"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["result"]["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(result["result"]["synced"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(result["propagation"]["state_name"].as_str(), Some("failed"));
    assert_eq!(result["propagation"]["last_sync_error"].as_str(), Some("remote fetch postponed"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 82, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("postponed remote fetch peer event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some(peer));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote fetch postponed")
    );
}
