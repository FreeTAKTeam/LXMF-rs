fn assert_local_remote_transfer_error_does_not_backoff_source_peer(
    method: &str,
    kind: std::io::ErrorKind,
) {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge { result: Err(kind) }));
    let peer = format!("peer-{method}-local-error");
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = true;
        record.sync_backoff = 60;
        record.last_sync_attempt = 321;
        record.next_sync_attempt = 654;
        record.acceptance_rate = 0.5;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            method,
            json!({
                "remote": peer,
                "identity_private_key_hex": "not-hex",
            }),
        ))
        .expect_err("local bridge failure should be returned");
    assert_eq!(err.kind(), kind);

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer.as_str()).expect("stored peer");
    assert!(record.alive);
    assert_eq!(record.sync_backoff, 60);
    assert_eq!(record.last_sync_attempt, 321);
    assert_eq!(record.next_sync_attempt, 654);
    assert_eq!(record.acceptance_rate, 0.5);
    drop(peers);

    assert!(
        daemon
            .event_queue
            .lock()
            .expect("event_queue mutex poisoned")
            .iter()
            .all(|event| event.event_type != "peer_sync"),
        "local bridge failures must not publish a failed peer sync event"
    );
}

#[test]
fn invalid_input_propagation_remote_download_does_not_backoff_source_peer() {
    assert_local_remote_transfer_error_does_not_backoff_source_peer(
        "propagation_remote_download",
        std::io::ErrorKind::InvalidInput,
    );
}

#[test]
fn local_setup_propagation_remote_fetch_error_does_not_backoff_source_peer() {
    assert_local_remote_transfer_error_does_not_backoff_source_peer(
        "propagation_remote_fetch",
        std::io::ErrorKind::Other,
    );
}

#[test]
fn failed_propagation_remote_fetch_prunes_stale_queue_snapshot_ids_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let peer = "peer-remote-fetch-fail-stale-snapshot";
    let stale_handled_id = "f6".repeat(32);
    let stale_unhandled_id = "f7".repeat(32);
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
        record.restored_handled_ids.push(stale_handled_id);
        record.restored_unhandled_ids.push(stale_unhandled_id);
    }
    let pending = PropagationEntryRecord {
        transient_id: "ba".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_621,
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

fn assert_denied_remote_transfer_breaks_source_peering(method: &str, peer: &str) {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteTransferErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node denied access",
        fail_download: method == "propagation_remote_download",
        fail_fetch: method == "propagation_remote_fetch",
    }));
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    let entry = PropagationEntryRecord {
        transient_id: "bb".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_622,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark peer unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            method,
            json!({
                "remote": peer,
            }),
        ))
        .expect_err("denied remote transfer should still return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node denied access");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 82, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        !peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .any(|row| row["peer"].as_str() == Some(peer)),
        "denied access should break local source peering"
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("list unhandled")
            .is_empty(),
        "denied access should clear source peer propagation queue marks"
    );

    let status = daemon
        .handle_rpc(RpcRequest {
            id: 83,
            method: "propagation_status".to_string(),
            params: None,
        })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0xf4));
    assert_eq!(status["propagation"]["state_name"].as_str(), Some("no_access"));
    assert_eq!(
        status["propagation"]["last_sync_error"].as_str(),
        Some("propagation node denied access")
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("denied access unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer));
    assert_eq!(event.payload["remote"].as_str(), Some(peer));
    assert_eq!(event.payload["reason"].as_str(), Some("access_denied"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
}

#[test]
fn denied_access_propagation_remote_download_breaks_source_peering_like_python() {
    assert_denied_remote_transfer_breaks_source_peering(
        "propagation_remote_download",
        "peer-remote-download-denied",
    );
}

#[test]
fn denied_access_propagation_remote_fetch_breaks_source_peering_like_python() {
    assert_denied_remote_transfer_breaks_source_peering(
        "propagation_remote_fetch",
        "peer-remote-fetch-denied",
    );
}

#[test]
fn denied_access_propagation_remote_fetch_reports_stored_peer_case_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteTransferErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node denied access",
        fail_download: false,
        fail_fetch: true,
    }));
    let stored_peer = "Peer-Remote-Fetch-Denied-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": stored_peer })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_remote_fetch",
            json!({
                "remote": request_peer,
            }),
        ))
        .expect_err("denied remote fetch should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("denied access unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some(stored_peer));
    assert_eq!(event.payload["remote"].as_str(), Some(request_peer.as_str()));
    assert_eq!(event.payload["reason"].as_str(), Some("access_denied"));
}

struct RemoteTransferFailureCase {
    suffix: &'static str,
    kind: std::io::ErrorKind,
    message: &'static str,
    failure_kind: &'static str,
}

fn assert_retryable_remote_transfer_failure_matches_sync_path(
    method: &str,
    case: &RemoteTransferFailureCase,
) {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteTransferErrorBridge {
        kind: case.kind,
        message: case.message,
        fail_download: method == "propagation_remote_download",
        fail_fetch: method == "propagation_remote_fetch",
    }));
    let method_suffix = method.strip_prefix("propagation_remote_").expect("remote method suffix");
    let peer = format!("peer-{method_suffix}-{}", case.suffix);
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = true;
        record.sync_backoff = 0;
        record.next_sync_attempt = 0;
        record.acceptance_rate = 0.62;
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let transient_id = format!("{:02x}", 0xd0 + (case.suffix.len() % 16)).repeat(32);
    let pending = PropagationEntryRecord {
        transient_id,
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_700 + i64::try_from(case.suffix.len()).expect("case suffix len"),
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), pending.transient_id.as_str())
        .expect("mark peer unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            method,
            json!({
                "remote": peer,
            }),
        ))
        .expect_err("retryable remote transfer failure should return the bridge error");
    assert_eq!(err.kind(), case.kind);
    assert_eq!(err.to_string(), case.message);

    let status = daemon
        .handle_rpc(RpcRequest { id: 82, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some(case.message));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 83, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer should remain queued for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.62));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "retryable remote transfer failure should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("failed remote transfer peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer.as_str()));
    assert_eq!(event.payload["remote"].as_str(), Some(peer.as_str()));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["failure_kind"].as_str(), Some(case.failure_kind));
    assert_eq!(
        event.payload["propagation"]["failure_kind"].as_str(),
        Some(case.failure_kind)
    );
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(event.payload["propagation"]["error"].as_str(), Some(case.message));
}

fn assert_retryable_remote_transfer_failures_match_sync_paths(method: &str) {
    for case in [
        RemoteTransferFailureCase {
            suffix: "timeout",
            kind: std::io::ErrorKind::TimedOut,
            message: "propagation peer timed out",
            failure_kind: "timeout",
        },
        RemoteTransferFailureCase {
            suffix: "closed-link",
            kind: std::io::ErrorKind::BrokenPipe,
            message: "propagation link closed",
            failure_kind: "timeout",
        },
        RemoteTransferFailureCase {
            suffix: "needs-id",
            kind: std::io::ErrorKind::PermissionDenied,
            message: "propagation node requires identity",
            failure_kind: "no_identity",
        },
        RemoteTransferFailureCase {
            suffix: "invalid-key",
            kind: std::io::ErrorKind::PermissionDenied,
            message: "propagation peer invalid peering key",
            failure_kind: "invalid_key",
        },
        RemoteTransferFailureCase {
            suffix: "invalid-stamp",
            kind: std::io::ErrorKind::PermissionDenied,
            message: "propagation peer invalid stamp",
            failure_kind: "invalid_stamp",
        },
        RemoteTransferFailureCase {
            suffix: "invalid-data",
            kind: std::io::ErrorKind::InvalidInput,
            message: "propagation node rejected the request",
            failure_kind: "invalid_data",
        },
        RemoteTransferFailureCase {
            suffix: "not-found",
            kind: std::io::ErrorKind::NotFound,
            message: "propagation peer not found",
            failure_kind: "not_found",
        },
    ] {
        assert_retryable_remote_transfer_failure_matches_sync_path(method, &case);
    }
}

#[test]
fn propagation_remote_download_classifies_retryable_bridge_failures_like_sync_paths() {
    assert_retryable_remote_transfer_failures_match_sync_paths("propagation_remote_download");
}

#[test]
fn propagation_remote_fetch_classifies_retryable_bridge_failures_like_sync_paths() {
    assert_retryable_remote_transfer_failures_match_sync_paths("propagation_remote_fetch");
}

fn assert_remote_transfer_bridge_unavailable_preserves_failed_lifecycle(method: &str) {
    let daemon = RpcDaemon::test_instance();
    let method_suffix = method.strip_prefix("propagation_remote_").expect("remote method suffix");
    let peer = format!("peer-{method_suffix}-bridge-unavailable");
    daemon
        .handle_rpc(rpc_request(80, "peer_sync", json!({ "peer": peer })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.restored_handled_ids.clear();
        record.restored_unhandled_ids.clear();
    }
    let pending = PropagationEntryRecord {
        transient_id: if method == "propagation_remote_download" {
            "e0".repeat(32)
        } else {
            "e1".repeat(32)
        },
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_740,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), pending.transient_id.as_str())
        .expect("mark peer unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            81,
            method,
            json!({
                "remote": peer,
            }),
        ))
        .expect_err("missing bridge should return unavailable error");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let status = daemon
        .handle_rpc(RpcRequest { id: 82, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
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

    let peers = daemon
        .handle_rpc(RpcRequest { id: 83, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer should remain listed");
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(pending.transient_id.as_str())]
    );
    assert!(
        daemon
            .event_queue
            .lock()
            .expect("event_queue mutex poisoned")
            .iter()
            .all(|event| event.event_type != "peer_unpeer"),
        "bridge unavailable should not break peering"
    );
}

#[test]
fn propagation_remote_download_bridge_unavailable_preserves_failed_lifecycle_and_queue() {
    assert_remote_transfer_bridge_unavailable_preserves_failed_lifecycle(
        "propagation_remote_download",
    );
}

#[test]
fn propagation_remote_fetch_bridge_unavailable_preserves_failed_lifecycle_and_queue() {
    assert_remote_transfer_bridge_unavailable_preserves_failed_lifecycle("propagation_remote_fetch");
}

#[test]
fn failed_propagation_remote_sync_updates_lifecycle_error() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));

    let err = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let status = daemon
        .handle_rpc(RpcRequest { id: 75, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("remote sync failed"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 76, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-a"))
        .expect("peer row");
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("failed remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-a"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("timeout"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("timeout"));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
}

#[test]
fn failed_propagation_remote_sync_updates_peer_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-remote-sync-fail" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-sync-fail").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-fail",
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-sync-fail"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 0.5));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("failed remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-sync-fail"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["failure_kind"].as_str(), Some("timeout"));
    assert_eq!(event.payload["propagation"]["failure_kind"].as_str(), Some("timeout"));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(event.payload["propagation"]["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["synced"].as_bool(), Some(false));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote sync failed")
    );
}
