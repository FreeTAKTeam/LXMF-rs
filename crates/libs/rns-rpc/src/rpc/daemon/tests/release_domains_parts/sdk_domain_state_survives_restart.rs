#[test]
fn sdk_domain_state_survives_restart() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).expect("unix epoch").as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-domain-{run_id}-{}.sqlite", std::process::id()));

    let topic_id: String;
    let attachment_id: String;
    let marker_id: String;
    let correlation_id: String;
    let session_id: String;

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open sqlite store");
        let daemon = RpcDaemon::with_store(store, "persist-node".to_string());

        let topic = daemon
            .handle_rpc(rpc_request(
                200,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/persist" }),
            ))
            .expect("topic create");
        assert!(topic.error.is_none());
        topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
            .as_str()
            .expect("topic id")
            .to_string();

        let subscribe = daemon
            .handle_rpc(rpc_request(
                201,
                "sdk_topic_subscribe_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("topic subscribe");
        assert!(subscribe.error.is_none());

        let publish = daemon
            .handle_rpc(rpc_request(
                202,
                "sdk_topic_publish_v2",
                json!({
                    "topic_id": topic_id.clone(),
                    "payload": { "message": "persist me" },
                }),
            ))
            .expect("topic publish");
        assert!(publish.error.is_none());

        let attachment = daemon
            .handle_rpc(rpc_request(
                203,
                "sdk_attachment_store_v2",
                json!({
                    "name": "persist.bin",
                    "content_type": "application/octet-stream",
                    "bytes_base64": "AQID",
                    "topic_ids": [topic_id.clone()],
                }),
            ))
            .expect("attachment store");
        assert!(attachment.error.is_none());
        attachment_id = attachment.result.expect("attachment result")["attachment"]
            ["attachment_id"]
            .as_str()
            .expect("attachment id")
            .to_string();

        let marker = daemon
            .handle_rpc(rpc_request(
                204,
                "sdk_marker_create_v2",
                json!({
                    "label": "Persist Marker",
                    "position": { "lat": 10.0, "lon": 10.0, "alt_m": null },
                    "topic_id": topic_id.clone(),
                }),
            ))
            .expect("marker create");
        assert!(marker.error.is_none());
        marker_id = marker.result.expect("marker result")["marker"]["marker_id"]
            .as_str()
            .expect("marker id")
            .to_string();

        let identity_bundle = json!({
            "identity": "persist-imported",
            "public_key": "persist-imported-pub",
            "display_name": "Persist Imported",
            "capabilities": ["ops"],
            "extensions": {},
        });
        let identity_import = daemon
            .handle_rpc(rpc_request(
                205,
                "sdk_identity_import_v2",
                json!({
                    "bundle_base64": BASE64_STANDARD.encode(identity_bundle.to_string().as_bytes()),
                }),
            ))
            .expect("identity import");
        assert!(identity_import.error.is_none());

        let identity_activate = daemon
            .handle_rpc(rpc_request(
                206,
                "sdk_identity_activate_v2",
                json!({ "identity": "persist-imported" }),
            ))
            .expect("identity activate");
        assert!(identity_activate.error.is_none());

        let command = daemon
            .handle_rpc(rpc_request(
                207,
                "sdk_command_invoke_v2",
                json!({
                    "command": "ping",
                    "target": "persist-imported",
                    "payload": { "hello": "world" },
                }),
            ))
            .expect("command invoke");
        assert!(command.error.is_none());
        correlation_id = command.result.expect("command result")["response"]["payload"]
            ["correlation_id"]
            .as_str()
            .expect("correlation_id")
            .to_string();

        let voice_open = daemon
            .handle_rpc(rpc_request(
                208,
                "sdk_voice_session_open_v2",
                json!({ "peer_id": "persist-imported", "codec_hint": "opus" }),
            ))
            .expect("voice open");
        assert!(voice_open.error.is_none());
        session_id = voice_open.result.expect("voice open result")["session_id"]
            .as_str()
            .expect("session_id")
            .to_string();

        let voice_update = daemon
            .handle_rpc(rpc_request(
                209,
                "sdk_voice_session_update_v2",
                json!({ "session_id": session_id.clone(), "state": "active" }),
            ))
            .expect("voice update");
        assert!(voice_update.error.is_none());
    }

    {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen sqlite store");
        let daemon = RpcDaemon::with_store(store, "persist-node".to_string());

        let topic_get = daemon
            .handle_rpc(rpc_request(
                210,
                "sdk_topic_get_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("topic get after restart");
        assert!(topic_get.error.is_none());
        assert_eq!(topic_get.result.expect("result")["topic"]["topic_id"], json!(topic_id.clone()));

        let telemetry = daemon
            .handle_rpc(rpc_request(
                211,
                "sdk_telemetry_query_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("telemetry after restart");
        assert!(telemetry.error.is_none());
        assert!(!telemetry.result.expect("result")["points"]
            .as_array()
            .expect("points array")
            .is_empty());

        let attachment_download = daemon
            .handle_rpc(rpc_request(
                212,
                "sdk_attachment_download_v2",
                json!({ "attachment_id": attachment_id.clone() }),
            ))
            .expect("attachment download after restart");
        assert!(attachment_download.error.is_none());
        assert_eq!(attachment_download.result.expect("result")["bytes_base64"], json!("AQID"));

        let marker_list = daemon
            .handle_rpc(rpc_request(
                213,
                "sdk_marker_list_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("marker list after restart");
        assert!(marker_list.error.is_none());
        let marker_result = marker_list.result.expect("result");
        let marker_rows = marker_result["markers"].as_array().expect("marker rows");
        assert!(marker_rows.iter().any(|row| row["marker_id"] == json!(marker_id.clone())));

        let identity_export = daemon
            .handle_rpc(rpc_request(
                214,
                "sdk_identity_export_v2",
                json!({ "identity": "persist-imported" }),
            ))
            .expect("identity export after restart");
        assert!(identity_export.error.is_none());

        let command_reply = daemon
            .handle_rpc(rpc_request(
                215,
                "sdk_command_reply_v2",
                json!({
                    "correlation_id": correlation_id.clone(),
                    "accepted": true,
                    "payload": { "reply": "pong" },
                }),
            ))
            .expect("command reply after restart");
        assert!(command_reply.error.is_none());
        let command_session = daemon
            .handle_rpc(rpc_request(
                2151,
                "sdk_command_session_get_v2",
                json!({ "correlation_id": correlation_id.clone() }),
            ))
            .expect("command session after restart");
        assert!(command_session.error.is_none());
        assert_eq!(
            command_session.result.expect("command session after restart result")["session"]
                ["command_state"],
            json!("completed")
        );

        let voice_close = daemon
            .handle_rpc(rpc_request(
                216,
                "sdk_voice_session_close_v2",
                json!({ "session_id": session_id.clone() }),
            ))
            .expect("voice close after restart");
        assert!(voice_close.error.is_none());

        let topic_2 = daemon
            .handle_rpc(rpc_request(
                217,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/persist-2" }),
            ))
            .expect("second topic create");
        assert!(topic_2.error.is_none());
        let topic_2_id = topic_2.result.expect("topic2 result")["topic"]["topic_id"]
            .as_str()
            .expect("topic2 id")
            .to_string();
        assert_ne!(topic_2_id, topic_id);
    }

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sdk_domain_state_is_storage_authoritative_across_live_daemons() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).expect("unix epoch").as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("lxmf-rs-sdk-authority-{run_id}-{}.sqlite", std::process::id()));

    let store_a = MessagesStore::open(db_path.as_path()).expect("open sqlite store A");
    let daemon_a = RpcDaemon::with_store(store_a, "authority-node".to_string());
    let store_b = MessagesStore::open(db_path.as_path()).expect("open sqlite store B");
    let daemon_b = RpcDaemon::with_store(store_b, "authority-node".to_string());

    let topic = daemon_a
        .handle_rpc(rpc_request(300, "sdk_topic_create_v2", json!({ "topic_path": "ops/shared" })))
        .expect("topic create");
    assert!(topic.error.is_none());
    let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
        .as_str()
        .expect("topic id")
        .to_string();

    let topic_get_from_b = daemon_b
        .handle_rpc(rpc_request(301, "sdk_topic_get_v2", json!({ "topic_id": topic_id.clone() })))
        .expect("topic get from daemon B");
    assert!(topic_get_from_b.error.is_none());
    assert_eq!(
        topic_get_from_b.result.expect("result")["topic"]["topic_id"],
        json!(topic_id.clone())
    );

    let marker = daemon_b
        .handle_rpc(rpc_request(
            302,
            "sdk_marker_create_v2",
            json!({
                "label": "Shared Marker",
                "position": { "lat": 12.0, "lon": 12.0, "alt_m": null },
                "topic_id": topic_id.clone(),
            }),
        ))
        .expect("marker create on daemon B");
    assert!(marker.error.is_none());
    let marker_id = marker.result.expect("marker result")["marker"]["marker_id"]
        .as_str()
        .expect("marker id")
        .to_string();

    let marker_list_from_a = daemon_a
        .handle_rpc(rpc_request(303, "sdk_marker_list_v2", json!({ "topic_id": topic_id.clone() })))
        .expect("marker list from daemon A");
    assert!(marker_list_from_a.error.is_none());
    let marker_result = marker_list_from_a.result.expect("result");
    let marker_rows = marker_result["markers"].as_array().expect("marker rows");
    assert!(marker_rows.iter().any(|row| row["marker_id"] == json!(marker_id)));

    let command = daemon_a
        .handle_rpc(rpc_request(
            304,
            "sdk_command_invoke_v2",
            json!({
                "command": "sync",
                "target": "peer-a",
                "payload": { "mode": "live" },
            }),
        ))
        .expect("command invoke on daemon A");
    assert!(command.error.is_none());
    let correlation_id = command.result.expect("command result")["response"]["payload"]
        ["correlation_id"]
        .as_str()
        .expect("correlation_id")
        .to_string();

    let command_reply_from_b = daemon_b
        .handle_rpc(rpc_request(
            305,
            "sdk_command_reply_v2",
            json!({
                "correlation_id": correlation_id,
                "accepted": true,
                "payload": { "reply": "ok" },
            }),
        ))
        .expect("command reply on daemon B");
    assert!(command_reply_from_b.error.is_none());

    let command_session_from_b = daemon_b
        .handle_rpc(rpc_request(
            306,
            "sdk_command_session_get_v2",
            json!({ "correlation_id": correlation_id.clone() }),
        ))
        .expect("command session get on daemon B");
    assert!(command_session_from_b.error.is_none());
    assert_eq!(
        command_session_from_b.result.expect("command session result")["session"]["response_payload"]
            ["reply"],
        json!("ok")
    );

    daemon_b
        .accept_inbound_for_test(MessageRecord {
            id: "cmd-live-daemon-complete".to_owned(),
            source: "peer-a".to_owned(),
            destination: "authority-node".to_owned(),
            title: "command complete".to_owned(),
            content: "ok".to_owned(),
            timestamp: now_i64(),
            direction: "in".to_owned(),
            fields: Some(json!({
                "sdk_command": {
                    "correlation_id": correlation_id,
                    "event": "completed",
                    "accepted": true,
                    "payload": { "reply": "from-inbound" }
                }
            })),
            receipt_status: None,
        })
        .expect("accept inbound correlation on daemon B");

    let command_session_after_inbound = daemon_b
        .handle_rpc(rpc_request(
            307,
            "sdk_command_session_get_v2",
            json!({ "correlation_id": correlation_id.clone() }),
        ))
        .expect("command session get after inbound");
    assert!(command_session_after_inbound.error.is_none());
    assert_eq!(
        command_session_after_inbound.result.expect("command session after inbound result")["session"]
            ["response_payload"]["reply"],
        json!("from-inbound")
    );

    let _ = std::fs::remove_file(&db_path);
}
