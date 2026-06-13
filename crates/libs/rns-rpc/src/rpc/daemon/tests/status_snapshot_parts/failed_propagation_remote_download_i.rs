#[test]
fn failed_propagation_remote_download_import_updates_source_peer_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "imported_count": 1,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));
    let peer = "peer-remote-download-import-fail-backoff";
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
        transient_id: "c7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_626,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_remote_download",
            json!({
                "remote": peer,
            }),
        ))
        .expect_err("remote download import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));

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
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
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
        .expect("failed remote download peer event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some(peer));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert!(event.payload["propagation"]["error"]
        .as_str()
        .is_some_and(|value| value.contains("invalid remote propagation payload hex")));
}

#[test]
fn failed_propagation_remote_fetch_import_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));
    let peer = "peer-remote-fetch-import-fail-snapshot";
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "b6".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_617,
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
            81,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect_err("remote fetch import failure should be returned");
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
fn failed_propagation_remote_fetch_import_updates_source_peer_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));
    let peer = "peer-remote-fetch-import-fail-backoff";
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
        transient_id: "c6".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_625,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_remote_fetch",
            json!({
                "remote": peer,
            }),
        ))
        .expect_err("remote fetch import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));

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
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
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
        .expect("failed remote fetch peer event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some(peer));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert!(event.payload["propagation"]["error"]
        .as_str()
        .is_some_and(|value| value.contains("invalid remote propagation payload hex")));
}

#[test]
fn failed_propagation_remote_fetch_records_existing_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-fetch-fail-snapshot";
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: "b9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_620,
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
            81,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect_err("remote fetch bridge failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
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
fn failed_propagation_remote_fetch_updates_source_peer_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-fetch-fail-backoff";
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
        transient_id: "bc".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_623,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_remote_fetch",
            json!({
                "remote": peer,
            }),
        ))
        .expect_err("remote fetch bridge failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

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
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
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
        .expect("failed remote fetch peer event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some(peer));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote fetch failed")
    );
}
