#[test]
fn failed_propagation_remote_unpeer_records_case_insensitive_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let stored_peer = "Peer-Remote-Unpeer-Fail-Snapshot-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "24".repeat(16),
        payload_hex: "24".repeat(20),
        received_at: 1_700_000_809,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": request_peer,
            }),
        ))
        .expect_err("remote unpeer failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
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
fn payload_backed_peer_queue_snapshot_uses_stored_peer_case_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Snapshot-Mixed-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": stored_peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(stored_peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "25".repeat(16),
        payload_hex: "25".repeat(20),
        received_at: 1_700_000_810,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(stored_peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    daemon
        .record_payload_backed_peer_queue_snapshot(request_peer.as_str())
        .expect("record queue snapshot");

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(stored_peer).expect("stored peer");
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
fn unavailable_propagation_remote_unpeer_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-remote-unpeer-unavailable-snapshot";
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }

    let entry = PropagationEntryRecord {
        transient_id: "e4".repeat(32),
        destination: "1c".repeat(16),
        payload_hex: "1c".repeat(20),
        received_at: 1_700_000_804,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": peer,
            }),
        ))
        .expect_err("missing bridge should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let status = daemon
        .handle_rpc(rpc_request(81, "propagation_status", JsonValue::Null))
        .expect("propagation status after unavailable remote unpeer bridge")
        .result
        .expect("status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(
        propagation["last_sync_error"].as_str(),
        Some("remote control bridge unavailable")
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
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn failed_propagation_remote_sync_clears_previous_completion() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));
    daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("initial remote sync");

    let completed = daemon
        .handle_rpc(RpcRequest { id: 77, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert!(completed["propagation"]["last_sync_completed"].as_i64().is_some());

    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let err = daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect_err("second remote sync should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let failed = daemon
        .handle_rpc(RpcRequest { id: 79, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &failed["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("remote sync failed"));
}

#[test]
fn propagation_acknowledge_sync_completion_resets_completed_state_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));
    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("remote sync");

    let acknowledged = daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_acknowledge_sync_completion",
            json!({}),
        ))
        .expect("acknowledge sync")
        .result
        .expect("acknowledge result");
    let propagation = &acknowledged["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x00));
    assert_eq!(propagation["state_name"].as_str(), Some("idle"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn propagation_acknowledge_sync_completion_preserves_failure_without_reset() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect_err("remote sync failure should be returned");

    let acknowledged = daemon
        .handle_rpc(rpc_request(
            83,
            "propagation_acknowledge_sync_completion",
            json!({}),
        ))
        .expect("acknowledge failed sync")
        .result
        .expect("acknowledge result");
    let propagation = &acknowledged["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));

    let reset = daemon
        .handle_rpc(rpc_request(
            84,
            "propagation_acknowledge_sync_completion",
            json!({ "reset_state": true }),
        ))
        .expect("reset failed sync")
        .result
        .expect("reset result");
    let propagation = &reset["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x00));
    assert_eq!(propagation["state_name"].as_str(), Some("idle"));
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn peer_types_drive_python_style_peer_counts() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            70,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static"],
            }),
        ))
        .expect("enable propagation");

    daemon
        .handle_rpc(rpc_request(71, "peer_sync", json!({ "peer": "peer-static" })))
        .expect("sync static peer");
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-manual" })))
        .expect("sync manual peer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 73, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row["peer_type"].as_str() == Some("static")));
    assert!(rows.iter().any(|row| row["peer_type"].as_str() == Some("manual")));
}

#[test]
fn peer_record_exists_can_include_hidden_unpeered_records() {
    let daemon = RpcDaemon::test_instance();
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        guard.insert(
            "Peer-Hidden-Rejoin".to_string(),
            daemon.transient_peer_record(
                "Peer-Hidden-Rejoin".to_string(),
                1_700_000_902,
                Vec::new(),
                None,
                None,
                Some("unpeered".to_string()),
            ),
        );
    }

    assert!(daemon.peer_record_exists("peer-hidden-rejoin", true));
    assert!(!daemon.peer_record_exists("peer-hidden-rejoin", false));
    assert!(!daemon.peer_record_exists("peer-hidden-missing", true));
}

#[test]
fn list_peers_static_type_tracks_current_static_peer_config() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-old"],
            }),
        ))
        .expect("enable old static peer");
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-old" })))
        .expect("sync old static peer");
    daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-new"],
            }),
        ))
        .expect("replace static peers");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let old = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-old"))
        .expect("old peer row");
    assert_eq!(old["peer_type"].as_str(), Some("manual"));
    assert_eq!(old["type"].as_str(), Some("discovered"));
}
