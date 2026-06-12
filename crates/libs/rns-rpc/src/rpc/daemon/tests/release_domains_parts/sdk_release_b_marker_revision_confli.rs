#[test]
fn sdk_release_b_marker_revision_conflicts_are_rejected_across_live_daemons() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).expect("unix epoch").as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-marker-conflict-{run_id}-{}.sqlite", std::process::id()));

    let store_a = MessagesStore::open(db_path.as_path()).expect("open sqlite store A");
    let daemon_a = RpcDaemon::with_store(store_a, "marker-conflict-node".to_string());
    let store_b = MessagesStore::open(db_path.as_path()).expect("open sqlite store B");
    let daemon_b = RpcDaemon::with_store(store_b, "marker-conflict-node".to_string());

    let marker = daemon_a
        .handle_rpc(rpc_request(
            350,
            "sdk_marker_create_v2",
            json!({
                "label": "CAS marker",
                "position": { "lat": 45.0, "lon": -122.0, "alt_m": null }
            }),
        ))
        .expect("marker create");
    assert!(marker.error.is_none());
    let marker_result = marker.result.expect("marker result");
    let marker_id = marker_result["marker"]["marker_id"].as_str().expect("marker_id").to_string();
    let revision_1 = marker_result["marker"]["revision"].as_u64().expect("revision_1");
    assert_eq!(revision_1, 1);

    let update_success = daemon_b
        .handle_rpc(rpc_request(
            351,
            "sdk_marker_update_position_v2",
            json!({
                "marker_id": marker_id.clone(),
                "expected_revision": revision_1,
                "position": { "lat": 46.0, "lon": -123.0, "alt_m": null }
            }),
        ))
        .expect("marker update success");
    assert!(update_success.error.is_none());
    let revision_2 = update_success.result.expect("update result")["marker"]["revision"]
        .as_u64()
        .expect("revision_2");
    assert_eq!(revision_2, 2);

    let stale_update = daemon_a
        .handle_rpc(rpc_request(
            352,
            "sdk_marker_update_position_v2",
            json!({
                "marker_id": marker_id.clone(),
                "expected_revision": revision_1,
                "position": { "lat": 47.0, "lon": -124.0, "alt_m": null }
            }),
        ))
        .expect("stale marker update");
    let stale_update_error = stale_update.error.expect("stale update error");
    assert_eq!(stale_update_error.code, "SDK_RUNTIME_CONFLICT");
    let stale_update_details = stale_update_error.details.expect("stale update details");
    assert_eq!(stale_update_details["expected_revision"], json!(revision_1));
    assert_eq!(stale_update_details["observed_revision"], json!(revision_2));

    let stale_delete = daemon_a
        .handle_rpc(rpc_request(
            353,
            "sdk_marker_delete_v2",
            json!({
                "marker_id": marker_id.clone(),
                "expected_revision": revision_1
            }),
        ))
        .expect("stale marker delete");
    let stale_delete_error = stale_delete.error.expect("stale delete error");
    assert_eq!(stale_delete_error.code, "SDK_RUNTIME_CONFLICT");
    let stale_delete_details = stale_delete_error.details.expect("stale delete details");
    assert_eq!(stale_delete_details["expected_revision"], json!(revision_1));
    assert_eq!(stale_delete_details["observed_revision"], json!(revision_2));

    let delete_success = daemon_a
        .handle_rpc(rpc_request(
            354,
            "sdk_marker_delete_v2",
            json!({
                "marker_id": marker_id.clone(),
                "expected_revision": revision_2
            }),
        ))
        .expect("marker delete success");
    assert!(delete_success.error.is_none());
    assert_eq!(delete_success.result.expect("delete result")["accepted"], json!(true));

    let delete_missing = daemon_b
        .handle_rpc(rpc_request(
            355,
            "sdk_marker_delete_v2",
            json!({
                "marker_id": marker_id,
                "expected_revision": revision_2
            }),
        ))
        .expect("delete missing marker");
    assert!(delete_missing.error.is_none());
    assert_eq!(delete_missing.result.expect("delete missing result")["accepted"], json!(false));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sdk_config_and_terminal_state_survive_restart_without_orphan_transitions() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).expect("unix epoch").as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-recovery-{run_id}-{}.sqlite", std::process::id()));

    let topic_id: String;
    let message_id = "recovery-pending-1";

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open sqlite store");
        let daemon = RpcDaemon::with_store(store, "recovery-node".to_string());

        let configure = daemon
            .handle_rpc(rpc_request(
                400,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "event_stream": { "max_poll_events": 64 },
                        "overflow_policy": "reject"
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none());
        assert_eq!(configure.result.expect("result")["revision"], json!(1));

        let topic = daemon
            .handle_rpc(rpc_request(
                401,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/recovery" }),
            ))
            .expect("topic create");
        assert!(topic.error.is_none());
        topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
            .as_str()
            .expect("topic id")
            .to_string();

        let receive = daemon
            .handle_rpc(rpc_request(
                402,
                "receive_message",
                json!({
                    "id": message_id,
                    "source": "source.recovery",
                    "destination": "destination.recovery",
                    "title": "",
                    "content": "pending message",
                    "fields": null
                }),
            ))
            .expect("receive_message");
        assert!(receive.error.is_none());

        let cancel = daemon
            .handle_rpc(rpc_request(
                403,
                "sdk_cancel_message_v2",
                json!({ "message_id": message_id }),
            ))
            .expect("cancel");
        assert!(cancel.error.is_none());
        assert_eq!(cancel.result.expect("result")["result"], json!("Accepted"));
    }

    {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen sqlite store");
        let daemon = RpcDaemon::with_store(store, "recovery-node".to_string());

        let snapshot = daemon
            .handle_rpc(rpc_request(404, "sdk_snapshot_v2", json!({ "include_counts": true })))
            .expect("snapshot");
        assert!(snapshot.error.is_none());
        assert_eq!(snapshot.result.expect("result")["config_revision"], json!(1));

        let poll_over_limit = daemon
            .handle_rpc(rpc_request(
                405,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 65
                }),
            ))
            .expect("poll over limit");
        assert_eq!(
            poll_over_limit.error.expect("error").code,
            "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED"
        );

        let topic_get = daemon
            .handle_rpc(rpc_request(406, "sdk_topic_get_v2", json!({ "topic_id": topic_id })))
            .expect("topic get");
        assert!(topic_get.error.is_none());

        let status = daemon
            .handle_rpc(rpc_request(407, "sdk_status_v2", json!({ "message_id": message_id })))
            .expect("status");
        assert!(status.error.is_none());
        assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("cancelled"));

        let second_cancel = daemon
            .handle_rpc(rpc_request(
                408,
                "sdk_cancel_message_v2",
                json!({ "message_id": message_id }),
            ))
            .expect("second cancel");
        assert_eq!(second_cancel.result.expect("result")["result"], json!("AlreadyTerminal"));
    }

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sdk_backup_restore_drill_recovers_snapshot_and_messages() {
    fn sqlite_sidecar_paths(path: &std::path::Path) -> [std::path::PathBuf; 2] {
        let file_name = path.file_name().expect("sqlite file name").to_string_lossy();
        [
            path.with_file_name(format!("{file_name}-wal")),
            path.with_file_name(format!("{file_name}-shm")),
        ]
    }

    fn checkpoint_sqlite_db(path: &std::path::Path) {
        let conn = rusqlite::Connection::open(path).expect("open sqlite for checkpoint");
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").expect("checkpoint sqlite db");
    }

    fn restore_sqlite_file(from: &std::path::Path, to: &std::path::Path) {
        let _ = std::fs::remove_file(to);
        for sidecar in sqlite_sidecar_paths(to) {
            let _ = std::fs::remove_file(sidecar);
        }
        std::fs::copy(from, to).expect("restore backup");
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).expect("unix epoch").as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-drill-{run_id}-{}.sqlite", std::process::id()));
    let backup_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-drill-{run_id}-{}.sqlite.backup", std::process::id()));
    let restored_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-drill-{run_id}-{}.sqlite.restored", std::process::id()));

    let baseline_topic_id: String;
    let baseline_message_id = "drill-baseline-msg-1";
    let drift_topic_id: String;
    let drift_message_id = "drill-drift-msg-1";

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open sqlite store");
        let daemon = RpcDaemon::with_store(store, "drill-node".to_string());
        let topic = daemon
            .handle_rpc(rpc_request(
                500,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/drill-baseline" }),
            ))
            .expect("create baseline topic");
        assert!(topic.error.is_none());
        baseline_topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
            .as_str()
            .expect("topic id")
            .to_string();

        let inbound = daemon
            .handle_rpc(rpc_request(
                501,
                "receive_message",
                json!({
                    "id": baseline_message_id,
                    "source": "source.baseline",
                    "destination": "destination.baseline",
                    "title": "",
                    "content": "baseline payload",
                    "fields": null
                }),
            ))
            .expect("baseline receive");
        assert!(inbound.error.is_none());
    }

    checkpoint_sqlite_db(db_path.as_path());
    std::fs::copy(db_path.as_path(), backup_path.as_path()).expect("copy backup");

    {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen sqlite store");
        let daemon = RpcDaemon::with_store(store, "drill-node".to_string());
        let drift_topic = daemon
            .handle_rpc(rpc_request(
                502,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/drill-drift" }),
            ))
            .expect("create drift topic");
        assert!(drift_topic.error.is_none());
        drift_topic_id = drift_topic.result.expect("topic result")["topic"]["topic_id"]
            .as_str()
            .expect("drift topic id")
            .to_string();

        let drift_inbound = daemon
            .handle_rpc(rpc_request(
                503,
                "receive_message",
                json!({
                    "id": drift_message_id,
                    "source": "source.drift",
                    "destination": "destination.drift",
                    "title": "",
                    "content": "drift payload",
                    "fields": null
                }),
            ))
            .expect("drift receive");
        assert!(drift_inbound.error.is_none());
    }

    restore_sqlite_file(backup_path.as_path(), restored_path.as_path());

    {
        let store =
            MessagesStore::open(restored_path.as_path()).expect("open restored sqlite store");
        let daemon = RpcDaemon::with_store(store, "drill-node".to_string());

        let baseline_topic = daemon
            .handle_rpc(rpc_request(
                504,
                "sdk_topic_get_v2",
                json!({ "topic_id": baseline_topic_id.clone() }),
            ))
            .expect("baseline topic after restore");
        assert!(baseline_topic.error.is_none());

        let topic_list = daemon
            .handle_rpc(rpc_request(505, "sdk_topic_list_v2", json!({ "limit": 64 })))
            .expect("topic list after restore");
        let topic_list_result = topic_list.result.expect("topic list result");
        let topic_rows = topic_list_result["topics"].as_array().expect("topic rows");
        assert!(topic_rows.iter().any(|row| row["topic_id"] == json!(baseline_topic_id)));
        assert!(
            !topic_rows.iter().any(|row| row["topic_id"] == json!(drift_topic_id)),
            "restored snapshot should not include post-backup drift topic"
        );

        let baseline_status = daemon
            .handle_rpc(rpc_request(
                506,
                "sdk_status_v2",
                json!({ "message_id": baseline_message_id }),
            ))
            .expect("baseline status after restore");
        assert!(
            baseline_status.result.expect("status result")["message"].is_object(),
            "baseline message should survive restore"
        );

        let drift_status = daemon
            .handle_rpc(rpc_request(
                507,
                "sdk_status_v2",
                json!({ "message_id": drift_message_id }),
            ))
            .expect("drift status after restore");
        assert!(
            drift_status.result.expect("status result")["message"].is_null(),
            "restored snapshot should not include post-backup drift message"
        );
    }

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&backup_path);
    let _ = std::fs::remove_file(&restored_path);
    for sidecar in sqlite_sidecar_paths(db_path.as_path()) {
        let _ = std::fs::remove_file(sidecar);
    }
    for sidecar in sqlite_sidecar_paths(backup_path.as_path()) {
        let _ = std::fs::remove_file(sidecar);
    }
    for sidecar in sqlite_sidecar_paths(restored_path.as_path()) {
        let _ = std::fs::remove_file(sidecar);
    }
}
