#[test]
fn propagation_remote_unpeer_rejects_blank_peer_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let unpeer_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls,
        unpeer_calls: Arc::clone(&unpeer_calls),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-blank-peer",
                "peer": "   ",
            }),
        ))
        .expect_err("blank remote-unpeer peer should be rejected");
    assert!(
        rejected.to_string().contains("peer is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(unpeer_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn propagation_remote_unpeer_rejects_blank_peer_without_bridge() {
    let daemon = RpcDaemon::test_instance();

    let rejected = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-blank-peer",
                "peer": "   ",
            }),
        ))
        .expect_err("blank remote-unpeer peer should be rejected before bridge lookup");
    assert!(
        rejected.to_string().contains("peer is required"),
        "unexpected rejection error: {rejected}"
    );
}

#[test]
fn propagation_remote_sync_updates_lifecycle_status() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    let result_propagation = &result["propagation"];
    assert_eq!(result_propagation["sync_state"].as_u64(), Some(0x07));
    assert_eq!(result_propagation["state_name"].as_str(), Some("completed"));
    assert_eq!(result_propagation["sync_progress"].as_f64(), Some(1.0));
    assert!(result_propagation["last_sync_started"].as_i64().is_some());
    assert!(result_propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(result_propagation["last_sync_error"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 73, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x07));
    assert_eq!(propagation["state_name"].as_str(), Some("completed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(1.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn propagation_remote_sync_updates_peer_runtime_state() {
    let payload = b"remote-sync-peer-accounting";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let peer_id = hex::encode([3u8; 16]);
    let daemon =
        RpcDaemon::with_store(MessagesStore::in_memory().expect("store"), hex::encode([2u8; 16]));
    daemon.set_remote_control_bridge(Arc::new(TransferLimitResultRemoteControlBridge {
        expected_sync_transfer_limit_kb: Some(42.5),
        result: json!({
            "synced": true,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        }),
    }));
    daemon
        .handle_rpc(rpc_request(73, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.name = Some("Remote Sync State".to_string());
        peer.name_source = Some("test".to_string());
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = 1_700_010_000;
        peer.acceptance_rate = 0.25;
        peer.propagation_transfer_limit = Some(100_000);
        peer.propagation_sync_limit = Some(84_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
    }

    let remote_sync = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer_id,
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(remote_sync["peer_sync"]["peer"].as_str(), Some(peer_id.as_str()));
    assert_eq!(remote_sync["peer_sync"]["remote_sync"].as_bool(), Some(true));
    assert_eq!(remote_sync["peer_sync"]["synced"].as_bool(), Some(true));
    assert_eq!(remote_sync["peer_sync"]["name"].as_str(), Some("Remote Sync State"));
    assert_eq!(remote_sync["peer_sync"]["name_source"].as_str(), Some("test"));
    assert_eq!(remote_sync["peer_sync"]["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(remote_sync["peer_sync"]["tx_bytes"].as_u64(), Some(0));
    assert_eq!(remote_sync["peer_sync"]["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(remote_sync["peer_sync"]["offered"].as_u64(), Some(0));
    assert_eq!(remote_sync["peer_sync"]["outgoing"].as_u64(), Some(0));
    assert_eq!(remote_sync["peer_sync"]["incoming"].as_u64(), Some(1));
    assert_eq!(remote_sync["peer_sync"]["sync_transfer_rate"].as_f64(), Some(0.0));
    let response_peering_key =
        remote_sync["peer_sync"]["peering_key"].as_u64().expect("response peering key");
    assert!(response_peering_key >= 1);
    assert_eq!(
        remote_sync["peer_sync"]["propagation"]["peering_key"].as_u64(),
        Some(response_peering_key)
    );
    assert_eq!(remote_sync["peer_sync"]["propagation_transfer_limit"].as_u64(), Some(100_000));
    assert_eq!(remote_sync["peer_sync"]["propagation_sync_limit"].as_u64(), Some(84_000));
    assert_eq!(remote_sync["peer_sync"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(remote_sync["peer_sync"]["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(remote_sync["peer_sync"]["propagation"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(remote_sync["peer_sync"]["propagation"]["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(remote_sync["peer_sync"]["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        remote_sync["peer_sync"]["propagation"]["rejected_ids"]
            .as_array()
            .expect("response rejected ids"),
        &[] as &[JsonValue]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 75, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer_id.as_str()))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert!(row["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(row["tx_bytes"].as_u64(), Some(0));
    assert_eq!(row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(0.0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.25));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer_id.as_str()));
    assert_eq!(event.payload["name"].as_str(), Some("Remote Sync State"));
    assert_eq!(event.payload["name_source"].as_str(), Some("test"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert!(event.payload["timestamp"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(true));
    assert_eq!(event.payload["type"].as_str(), Some("discovered"));
    assert_eq!(event.payload["state"].as_u64(), Some(0));
    assert_eq!(event.payload["sync_strategy"].as_u64(), Some(2));
    assert_eq!(event.payload["ler"].as_u64(), Some(0));
    assert_eq!(event.payload["network_distance"].as_u64(), Some(1));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_transfer_limit"].as_u64(), Some(100_000));
    assert_eq!(event.payload["propagation_sync_limit"].as_u64(), Some(84_000));
    assert_eq!(event.payload["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(
        event.payload["propagation_stamp_cost_flexibility"].as_u64(),
        Some(2)
    );
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(event.payload["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(event.payload["stamp_cost_flexibility"].as_u64(), Some(2));
    let peering_key = event.payload["peering_key"].as_u64().expect("peering key");
    assert!(peering_key >= 1);
    assert_eq!(event.payload["propagation"]["peering_key"].as_u64(), Some(peering_key));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["propagation"]["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(event.payload["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["rejected_ids"]
            .as_array()
            .expect("event rejected ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(event.payload["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(event.payload["tx_bytes"].as_u64(), Some(0));
    assert_eq!(event.payload["sync_transfer_rate"].as_f64(), Some(0.0));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(0));
    assert_eq!(event.payload["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        event.payload["messages"]["offered_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(event.payload["propagation"]["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["synced"].as_bool(), Some(true));
    assert_eq!(
        event.payload["propagation"]["imported_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        event.payload["propagation"]["duplicate_count"].as_u64(),
        Some(0)
    );
    assert_eq!(
        event.payload["propagation"]["transferred_bytes"].as_u64(),
        Some(payload.len() as u64)
    );
}

#[test]
fn propagation_remote_sync_success_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-success-live-queue-snapshot";
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [],
        })),
    }));
    daemon
        .handle_rpc(rpc_request(73, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "d2".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_732,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store pending entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("seed live queue mark");

    let remote_sync = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(remote_sync["peer_sync"]["synced"].as_bool(), Some(true));
    assert_eq!(
        remote_sync["peer_sync"]["messages"]["unhandled_ids"]
            .as_array()
            .expect("response unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
}
