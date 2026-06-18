fn restored_peer_record(
    peer: &str,
    handled_ids: Vec<String>,
    unhandled_ids: Vec<String>,
) -> PeerRecord {
    let mut record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_960,
        "alive": true,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored peer");
    record.restored_handled_ids = handled_ids;
    record.restored_unhandled_ids = unhandled_ids;
    record
}

fn peer_restart_db_path(temp: &tempfile::TempDir) -> std::path::PathBuf {
    temp.path().join("messages.sqlite")
}

fn peer_restart_daemon(db_path: &std::path::Path) -> RpcDaemon {
    RpcDaemon::with_store(MessagesStore::open(db_path).expect("open persistent store"), "test-identity".into())
}

fn peer_restart_entry(
    transient_pair: &str,
    destination_pair: &str,
    payload_pair: &str,
    size_bytes: u64,
    received_at: i64,
) -> PropagationEntryRecord {
    PropagationEntryRecord {
        transient_id: transient_pair.repeat(32),
        destination: destination_pair.repeat(16),
        payload_hex: payload_pair.repeat(size_bytes as usize),
        received_at,
        size_bytes,
        stamp_value: None,
    }
}

fn queue_peer_restart_entry(
    daemon: &RpcDaemon,
    peer: &str,
    entry: &PropagationEntryRecord,
) {
    daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
    daemon
        .handle_rpc(rpc_request(
            9_001,
            "set_outbound_propagation_node",
            json!({ "peer": peer }),
        ))
        .expect("queue persistent propagation for peer");
}

fn peer_restart_row(daemon: &RpcDaemon, peer: &str) -> JsonValue {
    let peers = daemon
        .handle_rpc(RpcRequest { id: 9_002, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .cloned()
        .expect("peer row")
}

fn assert_peer_restart_ids(
    daemon: &RpcDaemon,
    peer: &str,
    expected_handled: &[String],
    expected_unhandled: &[String],
) {
    let row = peer_restart_row(daemon, peer);
    let handled: Vec<JsonValue> = expected_handled.iter().map(|id| json!(id)).collect();
    let unhandled: Vec<JsonValue> = expected_unhandled.iter().map(|id| json!(id)).collect();
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("handled ids"),
        handled.as_slice()
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        unhandled.as_slice()
    );
}

#[test]
fn list_peers_replays_restored_unhandled_queue_snapshot_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-unhandled-replay";
    let entry = PropagationEntryRecord {
        transient_id: "a1".repeat(32),
        destination: "31".repeat(16),
        payload_hex: "31".repeat(24),
        received_at: 1_700_000_961,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    let mixed_case = entry.transient_id.to_ascii_uppercase();
    daemon.peers.lock().expect("peers mutex poisoned").insert(
        peer.to_string(),
        restored_peer_record(
            peer,
            Vec::new(),
            vec![format!("  {mixed_case}  "), entry.transient_id.clone()],
        ),
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 101, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("restored peer row");

    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        daemon.store.list_peer_unhandled_propagation_ids(peer).expect("live unhandled ids"),
        vec![entry.transient_id.clone()]
    );
}

#[test]
fn list_peers_keeps_restored_handled_ids_from_reopening_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-handled-wins";
    let entry = PropagationEntryRecord {
        transient_id: "a2".repeat(32),
        destination: "32".repeat(16),
        payload_hex: "32".repeat(28),
        received_at: 1_700_000_962,
        size_bytes: 28,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    let mixed_case = entry.transient_id.to_ascii_uppercase();
    daemon.peers.lock().expect("peers mutex poisoned").insert(
        peer.to_string(),
        restored_peer_record(
            peer,
            vec![format!(" {mixed_case} ")],
            vec![entry.transient_id.clone()],
        ),
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 102, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("restored peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("live handled ids"),
        vec![entry.transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("live unhandled entries")
            .is_empty()
    );
}

#[test]
fn list_peers_prunes_missing_restored_snapshot_ids_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-missing-prune";
    let handled = PropagationEntryRecord {
        transient_id: "a3".repeat(32),
        destination: "33".repeat(16),
        payload_hex: "33".repeat(20),
        received_at: 1_700_000_963,
        size_bytes: 20,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "a4".repeat(32),
        destination: "34".repeat(16),
        payload_hex: "34".repeat(22),
        received_at: 1_700_000_964,
        size_bytes: 22,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon.peers.lock().expect("peers mutex poisoned").insert(
        peer.to_string(),
        restored_peer_record(
            peer,
            vec!["a5".repeat(32), handled.transient_id.to_ascii_uppercase()],
            vec![
                "a6".repeat(32),
                unhandled.transient_id.clone(),
                unhandled.transient_id.to_ascii_uppercase(),
            ],
        ),
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 103, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("restored peer row");
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    assert_eq!(record.restored_handled_ids, vec![handled.transient_id]);
    assert_eq!(record.restored_unhandled_ids, vec![unhandled.transient_id]);
}

#[test]
fn peer_restart_remote_transfer_lifecycle_preserves_local_queue_work() {
    for (method, params) in [
        ("propagation_remote_fetch", json!({ "remote": "remote-fetch-restart" })),
        ("propagation_remote_download", json!({ "remote": "remote-download-restart" })),
        (
            "propagation_remote_sync",
            json!({ "remote": "remote-sync-restart", "peer": "peer-restart-live-queue" }),
        ),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = peer_restart_db_path(&temp);
        let peer = "peer-restart-live-queue";
        let pending = peer_restart_entry("c1", "41", "41", 24, 1_700_001_001);

        {
            let daemon = peer_restart_daemon(db_path.as_path());
            daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
                result: Ok(json!({
                    "synced": true,
                    "downloaded_count": 0,
                    "fetched_count": 0,
                    "messages": [],
                })),
            }));
            queue_peer_restart_entry(&daemon, peer, &pending);

            daemon
                .handle_rpc(rpc_request(9_003, method, params))
                .expect("remote transfer should keep local relay work queued");
            assert_eq!(
                daemon
                    .store
                    .list_peer_unhandled_propagation(peer)
                    .expect("pending before restart"),
                vec![pending.clone()]
            );
        }

        let reloaded = peer_restart_daemon(db_path.as_path());
        reloaded
            .handle_rpc(rpc_request(
                9_004,
                "set_outbound_propagation_node",
                json!({ "peer": peer }),
            ))
            .expect("recreate peer after restart");
        assert_peer_restart_ids(&reloaded, peer, &[], std::slice::from_ref(&pending.transient_id));
        assert_eq!(
            reloaded
                .store
                .list_peer_unhandled_propagation(peer)
                .expect("pending after restart"),
            vec![pending]
        );
    }
}

#[test]
fn peer_restart_completed_source_marks_survive_remote_fetch_without_reopening() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = peer_restart_db_path(&temp);
    let source_peer = "peer-restart-fetch-source";
    let relay_peer = "peer-restart-fetch-relay";
    let payload = b"peer-restart-fetch-source-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));

    {
        let daemon = peer_restart_daemon(db_path.as_path());
        daemon
            .handle_rpc(rpc_request(9_005, "peer_sync", json!({ "peer": source_peer })))
            .expect("seed source peer");
        daemon
            .handle_rpc(rpc_request(9_006, "peer_sync", json!({ "peer": relay_peer })))
            .expect("seed relay peer");
        daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
            result: Ok(json!({
                "available_count": 1,
                "fetched_count": 1,
                "messages": [{
                    "transient_id": transient_id,
                    "payload_hex": payload_hex,
                }],
            })),
        }));

        daemon
            .handle_rpc(rpc_request(
                9_007,
                "propagation_remote_fetch",
                json!({ "remote": source_peer }),
            ))
            .expect("remote fetch marks source handled");
        assert_eq!(
            daemon.store.list_peer_handled_propagation_ids(source_peer).expect("source handled"),
            vec![transient_id.clone()]
        );
        assert!(
            daemon
                .store
                .list_peer_unhandled_propagation(source_peer)
                .expect("source unhandled")
                .is_empty()
        );
    }

    let reloaded = peer_restart_daemon(db_path.as_path());
    reloaded
        .handle_rpc(rpc_request(9_008, "peer_sync", json!({ "peer": source_peer })))
        .expect("recreate source peer");
    reloaded
        .handle_rpc(rpc_request(9_009, "peer_sync", json!({ "peer": relay_peer })))
        .expect("recreate relay peer");
    assert_peer_restart_ids(&reloaded, source_peer, std::slice::from_ref(&transient_id), &[]);
    assert_peer_restart_ids(&reloaded, relay_peer, &[], std::slice::from_ref(&transient_id));
}

#[test]
fn peer_restart_prunes_missing_payload_ids_from_reloaded_store_marks() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = peer_restart_db_path(&temp);
    let peer = "peer-restart-missing-store-prune";
    let handled = peer_restart_entry("c2", "42", "42", 20, 1_700_001_002);
    let unhandled = peer_restart_entry("c3", "43", "43", 22, 1_700_001_003);

    {
        let daemon = peer_restart_daemon(db_path.as_path());
        daemon.store.upsert_propagation_entry(&handled).expect("store handled");
        daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled");
        daemon
            .store
            .mark_peer_handled_propagation(peer, handled.transient_id.as_str())
            .expect("mark handled");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, unhandled.transient_id.as_str())
            .expect("mark unhandled");
        daemon
            .store
            .mark_peer_handled_propagation(peer, "c4".repeat(32).as_str())
            .expect("mark missing handled");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, "c5".repeat(32).as_str())
            .expect("mark missing unhandled");
    }

    let reloaded = peer_restart_daemon(db_path.as_path());
    reloaded
        .handle_rpc(rpc_request(9_010, "peer_sync", json!({ "peer": peer })))
        .expect("recreate peer after restart");
    assert_peer_restart_ids(
        &reloaded,
        peer,
        std::slice::from_ref(&handled.transient_id),
        std::slice::from_ref(&unhandled.transient_id),
    );
    assert_eq!(
        reloaded.store.list_peer_handled_propagation_ids(peer).expect("handled after restart"),
        vec![handled.transient_id]
    );
    assert_eq!(
        reloaded.store.list_peer_unhandled_propagation_ids(peer).expect("unhandled after restart"),
        vec![unhandled.transient_id]
    );
}

#[test]
fn peer_restart_failed_lifecycle_remains_inspectable_without_queue_corruption() {
    for (method, bridge, expected_state, expected_name, expected_error) in [
        (
            "propagation_remote_download",
            Some(Arc::new(RemoteTransferErrorBridge {
                kind: std::io::ErrorKind::PermissionDenied,
                message: "propagation node denied access",
                fail_download: true,
                fail_fetch: false,
            }) as Arc<dyn RemoteControlBridge>),
            0xf4,
            "no_access",
            "propagation node denied access",
        ),
        (
            "propagation_remote_fetch",
            None,
            0xfe,
            "failed",
            "remote control bridge unavailable",
        ),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = peer_restart_db_path(&temp);
        let peer = "peer-restart-failed-queue";
        let pending = peer_restart_entry("c6", "46", "46", 18, 1_700_001_006);

        {
            let daemon = peer_restart_daemon(db_path.as_path());
            if let Some(bridge) = bridge.clone() {
                daemon.set_remote_control_bridge(bridge);
            }
            queue_peer_restart_entry(&daemon, peer, &pending);

            let err = daemon
                .handle_rpc(rpc_request(
                    9_011,
                    method,
                    json!({ "remote": "remote-restart-failure" }),
                ))
                .expect_err("remote lifecycle failure should be returned");
            assert_eq!(err.to_string(), expected_error);

            let status = daemon
                .handle_rpc(RpcRequest {
                    id: 9_012,
                    method: "propagation_status".to_string(),
                    params: None,
                })
                .expect("propagation status")
                .result
                .expect("propagation status result");
            assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(expected_state));
            assert_eq!(status["propagation"]["state_name"].as_str(), Some(expected_name));
            assert_eq!(status["propagation"]["last_sync_error"].as_str(), Some(expected_error));
            assert_eq!(
                daemon
                    .store
                    .list_peer_unhandled_propagation(peer)
                    .expect("pending before restart"),
                vec![pending.clone()]
            );
        }

        let reloaded = peer_restart_daemon(db_path.as_path());
        reloaded
            .handle_rpc(rpc_request(
                9_013,
                "set_outbound_propagation_node",
                json!({ "peer": peer }),
            ))
            .expect("recreate failed peer queue");
        assert_peer_restart_ids(&reloaded, peer, &[], std::slice::from_ref(&pending.transient_id));
        assert_eq!(
            reloaded
                .store
                .list_peer_unhandled_propagation(peer)
                .expect("pending after restart"),
            vec![pending]
        );
    }
}

#[test]
fn restart_reloads_serialized_restored_queue_snapshot_before_list_peers() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restart-serialized-reload";
    let handled = PropagationEntryRecord {
        transient_id: "a7".repeat(32),
        destination: "37".repeat(16),
        payload_hex: "37".repeat(26),
        received_at: 1_700_000_965,
        size_bytes: 26,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "a8".repeat(32),
        destination: "38".repeat(16),
        payload_hex: "38".repeat(30),
        received_at: 1_700_000_966,
        size_bytes: 30,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");

    let snapshot: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_967,
        "alive": true,
        "handled_ids": [format!(" {} ", handled.transient_id.to_ascii_uppercase())],
        "unhandled_ids": [
            "a9".repeat(32),
            unhandled.transient_id.to_ascii_uppercase(),
            handled.transient_id.clone(),
            unhandled.transient_id.clone(),
        ],
    }))
    .expect("deserialize peer snapshot");
    let serialized = serde_json::to_value(&snapshot).expect("serialize peer snapshot");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[
            json!("a9".repeat(32)),
            json!(unhandled.transient_id.as_str()),
            json!(handled.transient_id.as_str()),
            json!(unhandled.transient_id.as_str()),
        ]
    );

    let reloaded: PeerRecord =
        serde_json::from_value(serialized).expect("reload serialized peer snapshot");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), reloaded);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 104, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer))
        .expect("reloaded peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["offered_bytes"].as_u64(), Some(26));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(30));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("live handled ids"),
        vec![handled.transient_id.clone()]
    );
    assert_eq!(
        daemon.store.list_peer_unhandled_propagation_ids(peer).expect("live unhandled ids"),
        vec![unhandled.transient_id.clone()]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer).expect("stored peer");
    assert_eq!(record.restored_handled_ids, vec![handled.transient_id]);
    assert_eq!(record.restored_unhandled_ids, vec![unhandled.transient_id]);
}
