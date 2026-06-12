#[test]
fn failed_propagation_remote_sync_import_updates_peer_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": "peer-remote-sync-import-fail" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-sync-import-fail").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-import-fail",
            }),
        ))
        .expect_err("remote sync import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 80, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-sync-import-fail"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 0.5));
}

#[test]
fn failed_propagation_remote_sync_import_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));
    let peer = "peer-remote-import-fail-snapshot";
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "b5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_616,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("remote sync import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("pending propagation"),
        vec![pending.clone()]
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

#[test]
fn propagation_remote_unpeer_clears_local_peer_and_queue_state() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(76, "peer_sync", json!({ "peer": "peer-remote-unpeer" })))
        .expect("peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "e1".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(24),
        received_at: 1_700_000_801,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-remote-unpeer", entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-in".to_string(),
            source: "peer-remote-unpeer".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_802,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store inbound message");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-out".to_string(),
            source: "local".to_string(),
            destination: "peer-remote-unpeer".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_803,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound message");

    let result = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer",
            }),
        ))
        .expect("remote unpeer")
        .result
        .expect("remote unpeer result");
    assert_eq!(result["removed"].as_bool(), Some(true));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(24));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(2));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(result["offered"].as_u64(), Some(1));
    assert_eq!(result["outgoing"].as_u64(), Some(1));
    assert_eq!(result["incoming"].as_u64(), Some(1));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 78, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-remote-unpeer")
            .expect("list unhandled")
            .is_empty()
    );
}

#[test]
fn propagation_remote_unpeer_publishes_peer_removed_event() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": "peer-remote-unpeer-event" })))
        .expect("peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-event-in".to_string(),
            source: "peer-remote-unpeer-event".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_804,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store inbound message");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-event-out".to_string(),
            source: "local".to_string(),
            destination: "peer-remote-unpeer-event".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_805,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound message");

    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer-event",
            }),
        ))
        .expect("remote unpeer");

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-unpeer-event"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(event.payload["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["incoming"].as_u64(), Some(1));
}

#[test]
fn successful_propagation_remote_unpeer_clears_stale_lifecycle_failure() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": "peer-remote-unpeer-stale" })))
        .expect("peer sync");

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer-stale",
            }),
        ))
        .expect_err("first remote unpeer should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let failed_status = daemon
        .handle_rpc(RpcRequest {
            id: 81,
            method: "propagation_status".to_string(),
            params: None,
        })
        .expect("failed propagation status")
        .result
        .expect("failed propagation status result");
    assert_eq!(failed_status["propagation"]["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(failed_status["propagation"]["state_name"].as_str(), Some("failed"));
    assert_eq!(
        failed_status["propagation"]["last_sync_error"].as_str(),
        Some("remote unpeer failed")
    );

    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer-stale",
            }),
        ))
        .expect("successful remote unpeer");

    let status = daemon
        .handle_rpc(RpcRequest {
            id: 83,
            method: "propagation_status".to_string(),
            params: None,
        })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x00));
    assert_eq!(propagation["state_name"].as_str(), Some("idle"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn successful_propagation_remote_unpeer_preserves_newer_active_lifecycle() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-unpeer-active";
    daemon
        .handle_rpc(rpc_request(84, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync");
    daemon
        .handle_rpc(rpc_request(
            85,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("first remote unpeer should fail");

    daemon.update_propagation_sync_state(|state| {
        state.sync_state = 0x04;
        state.state_name = "syncing".to_string();
        state.sync_progress = 0.25;
        state.last_sync_started = Some(1_700_001_234);
        state.last_sync_completed = None;
        state.last_sync_error = None;
    });
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(
            86,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect("successful remote unpeer");

    let propagation = daemon.current_propagation_state();
    assert_eq!(propagation.sync_state, 0x04);
    assert_eq!(propagation.state_name, "syncing");
    assert_eq!(propagation.sync_progress, 0.25);
    assert_eq!(propagation.last_sync_started, Some(1_700_001_234));
    assert_eq!(propagation.last_sync_completed, None);
    assert_eq!(propagation.last_sync_error, None);
}
