#[test]
fn sdk_release_b_domain_methods_roundtrip() {
    let daemon = RpcDaemon::test_instance();

    let topic = daemon
        .handle_rpc(rpc_request(
            90,
            "sdk_topic_create_v2",
            json!({
                "topic_path": "ops/alerts",
                "metadata": { "kind": "ops" },
                "extensions": { "scope": "test" }
            }),
        ))
        .expect("topic create");
    assert!(topic.error.is_none());
    let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
        .as_str()
        .expect("topic id")
        .to_string();

    let topic_get = daemon
        .handle_rpc(rpc_request(91, "sdk_topic_get_v2", json!({ "topic_id": topic_id.clone() })))
        .expect("topic get");
    assert!(topic_get.error.is_none());
    assert_eq!(topic_get.result.expect("result")["topic"]["topic_path"], json!("ops/alerts"));

    let topic_list = daemon
        .handle_rpc(rpc_request(92, "sdk_topic_list_v2", json!({ "limit": 10 })))
        .expect("topic list");
    assert!(topic_list.error.is_none());
    assert_eq!(
        topic_list.result.expect("result")["topics"].as_array().expect("topic array").len(),
        1
    );

    let topic_subscribe = daemon
        .handle_rpc(rpc_request(
            93,
            "sdk_topic_subscribe_v2",
            json!({ "topic_id": topic_id.clone() }),
        ))
        .expect("topic subscribe");
    assert!(topic_subscribe.error.is_none());
    assert_eq!(topic_subscribe.result.expect("result")["accepted"], json!(true));

    let publish = daemon
        .handle_rpc(rpc_request(
            94,
            "sdk_topic_publish_v2",
            json!({
                "topic_id": topic_id.clone(),
                "payload": { "message": "hello topic" },
                "correlation_id": "corr-1"
            }),
        ))
        .expect("topic publish");
    assert!(publish.error.is_none());
    assert_eq!(publish.result.expect("result")["accepted"], json!(true));

    let telemetry = daemon
        .handle_rpc(rpc_request(
            95,
            "sdk_telemetry_query_v2",
            json!({ "topic_id": topic_id.clone() }),
        ))
        .expect("telemetry query");
    assert!(telemetry.error.is_none());
    assert!(!telemetry.result.expect("result")["points"]
        .as_array()
        .expect("points array")
        .is_empty());

    let attachment = daemon
        .handle_rpc(rpc_request(
            96,
            "sdk_attachment_store_v2",
            json!({
                "name": "sample.txt",
                "content_type": "text/plain",
                "bytes_base64": "aGVsbG8gd29ybGQ=",
                "topic_ids": [topic_id.clone()]
            }),
        ))
        .expect("attachment store");
    assert!(attachment.error.is_none());
    let attachment_id = attachment.result.expect("result")["attachment"]["attachment_id"]
        .as_str()
        .expect("attachment id")
        .to_string();

    let attachment_get = daemon
        .handle_rpc(rpc_request(
            97,
            "sdk_attachment_get_v2",
            json!({ "attachment_id": attachment_id }),
        ))
        .expect("attachment get");
    assert!(attachment_get.error.is_none());
    assert_eq!(attachment_get.result.expect("result")["attachment"]["name"], json!("sample.txt"));

    let attachment_list = daemon
        .handle_rpc(rpc_request(
            98,
            "sdk_attachment_list_v2",
            json!({ "topic_id": topic_id.clone() }),
        ))
        .expect("attachment list");
    assert!(attachment_list.error.is_none());
    assert_eq!(
        attachment_list.result.expect("result")["attachments"]
            .as_array()
            .expect("attachments array")
            .len(),
        1
    );

    let marker = daemon
        .handle_rpc(rpc_request(
            99,
            "sdk_marker_create_v2",
            json!({
                "label": "Alpha",
                "position": { "lat": 35.0, "lon": -115.0, "alt_m": 1200.0 },
                "topic_id": topic_id.clone()
            }),
        ))
        .expect("marker create");
    assert!(marker.error.is_none());
    let marker_result = marker.result.expect("result");
    let marker_id = marker_result["marker"]["marker_id"].as_str().expect("marker id").to_string();
    let marker_revision = marker_result["marker"]["revision"].as_u64().expect("marker revision");

    let marker_update = daemon
        .handle_rpc(rpc_request(
            100,
            "sdk_marker_update_position_v2",
            json!({
                "marker_id": marker_id,
                "expected_revision": marker_revision,
                "position": { "lat": 36.0, "lon": -116.0, "alt_m": null }
            }),
        ))
        .expect("marker update");
    assert!(marker_update.error.is_none());
    assert_eq!(marker_update.result.expect("result")["marker"]["position"]["lat"], json!(36.0));
}

#[test]
fn sdk_cursor_hint_returns_latest_non_null_cursor_for_method() {
    let daemon = RpcDaemon::test_instance();

    for (id, topic_path) in [(900_u64, "ops/alpha"), (901_u64, "ops/bravo")] {
        let created = daemon
            .handle_rpc(rpc_request(
                id,
                "sdk_topic_create_v2",
                json!({
                    "topic_path": topic_path,
                    "metadata": { "kind": "ops" },
                }),
            ))
            .expect("topic create");
        assert!(created.error.is_none());
    }

    let paged = daemon
        .handle_rpc(rpc_request(902, "sdk_topic_list_v2", json!({ "limit": 1 })))
        .expect("paged topic list");
    assert!(paged.error.is_none());
    let first_cursor = paged.result.expect("paged result")["next_cursor"]
        .as_str()
        .expect("first cursor")
        .to_string();

    let hinted = daemon
        .handle_rpc(rpc_request(
            903,
            "sdk_cursor_hint_v2",
            json!({ "method": "sdk_topic_list_v2" }),
        ))
        .expect("cursor hint");
    assert!(hinted.error.is_none());
    let hinted_result = hinted.result.expect("hint result");
    assert_eq!(hinted_result["method"], json!("sdk_topic_list_v2"));
    assert_eq!(hinted_result["hint"]["method"], json!("sdk_topic_list_v2"));
    assert_eq!(hinted_result["hint"]["next_cursor"], json!(first_cursor.clone()));
    assert!(hinted_result["hint"]["captured_at_ms"].as_u64().is_some());

    let terminal = daemon
        .handle_rpc(rpc_request(904, "sdk_topic_list_v2", json!({ "limit": 10 })))
        .expect("terminal topic list");
    assert!(terminal.error.is_none());
    assert_eq!(terminal.result.expect("terminal result")["next_cursor"], JsonValue::Null);

    let retained = daemon
        .handle_rpc(rpc_request(
            905,
            "sdk_cursor_hint_v2",
            json!({ "method": "sdk_topic_list_v2" }),
        ))
        .expect("retained cursor hint");
    assert!(retained.error.is_none());
    assert_eq!(retained.result.expect("retained result")["hint"]["next_cursor"], json!(first_cursor));
}

#[test]
fn sdk_domain_snapshot_restore_accepts_legacy_remote_command_arrays() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    store
        .put_sdk_domain_snapshot(&json!({
            "next_domain_seq": 7,
            "topics": {},
            "topic_order": [],
            "topic_subscriptions": [],
            "telemetry_points": [],
            "attachments": {},
            "attachment_payloads": {},
            "attachment_order": [],
            "markers": {},
            "marker_order": [],
            "identities": {},
            "contacts": {},
            "contact_order": [],
            "active_identity": null,
            "remote_commands": ["cmd-legacy-1", "cmd-legacy-2"],
            "voice_sessions": {},
        }))
        .expect("persist legacy snapshot");

    let daemon = RpcDaemon::with_store(store, "legacy-restore-node".to_string());

    let command_sessions = daemon
        .handle_rpc(rpc_request(
            499,
            "sdk_command_session_list_v2",
            json!({ "limit": 10 }),
        ))
        .expect("command session list");
    assert!(command_sessions.error.is_none());
    assert_eq!(
        command_sessions.result.expect("command session list result")["session_list"]["sessions"]
            .as_array()
            .expect("session rows")
            .len(),
        0
    );
}

#[test]
fn sdk_remote_command_sessions_correlate_inbound_messages() {
    let daemon = RpcDaemon::test_instance();

    let command = daemon
        .handle_rpc(rpc_request(
            500,
            "sdk_command_invoke_v2",
            json!({
                "command": "ping",
                "target": "node-b",
                "payload": { "body": "hello" },
            }),
        ))
        .expect("command invoke");
    assert!(command.error.is_none());
    let correlation_id = command.result.expect("command result")["response"]["payload"]
        ["correlation_id"]
        .as_str()
        .expect("correlation_id")
        .to_string();

    let pre_poll = daemon
        .handle_rpc(rpc_request(
            501,
            "sdk_poll_events_v2",
            json!({ "cursor": null, "max": 100 }),
        ))
        .expect("pre poll");
    assert!(pre_poll.error.is_none());
    let pre_cursor = pre_poll.result.expect("pre poll result")["next_cursor"]
        .as_str()
        .expect("pre cursor")
        .to_string();

    daemon
        .accept_inbound_for_test(MessageRecord {
            id: "cmd-progress-1".to_owned(),
            source: "node-b".to_owned(),
            destination: "test-identity".to_owned(),
            title: "command progress".to_owned(),
            content: "processing".to_owned(),
            timestamp: now_i64(),
            direction: "in".to_owned(),
            fields: Some(json!({
                "sdk_command": {
                    "correlation_id": correlation_id,
                    "event": "processing_started",
                    "payload": { "stage": "decode" }
                }
            })),
            receipt_status: None,
        })
        .expect("accept inbound progress");

    daemon
        .accept_inbound_for_test(MessageRecord {
            id: "cmd-complete-1".to_owned(),
            source: "node-b".to_owned(),
            destination: "test-identity".to_owned(),
            title: "command complete".to_owned(),
            content: "done".to_owned(),
            timestamp: now_i64(),
            direction: "in".to_owned(),
            fields: Some(json!({
                "sdk_command": {
                    "correlation_id": correlation_id,
                    "event": "completed",
                    "accepted": true,
                    "payload": { "reply": "pong" }
                }
            })),
            receipt_status: None,
        })
        .expect("accept inbound completion");

    let session = daemon
        .handle_rpc(rpc_request(
            502,
            "sdk_command_session_get_v2",
            json!({ "correlation_id": correlation_id.clone() }),
        ))
        .expect("command session get");
    assert!(session.error.is_none());
    let session_result = session.result.expect("session result");
    let command_id = session_result["session"]["command_id"].clone();
    assert_eq!(session_result["session"]["command_state"], json!("completed"));
    assert_eq!(session_result["session"]["accepted"], json!(true));
    assert_eq!(session_result["session"]["response_payload"]["reply"], json!("pong"));

    let poll = daemon
        .handle_rpc(rpc_request(
            503,
            "sdk_poll_events_v2",
            json!({ "cursor": pre_cursor, "max": 100 }),
        ))
        .expect("poll events");
    assert!(poll.error.is_none());
    let poll_result = poll.result.expect("poll result");
    let events = poll_result["events"].as_array().expect("events");
    assert!(events.iter().any(|event| {
        event["event_type"] == json!("command.processing_started")
            && event["payload"]["command_id"] == command_id
    }));
    assert!(events.iter().any(|event| {
        event["event_type"] == json!("command.completed")
            && event["payload"]["command_id"] == command_id
            && event["payload"]["response_payload"]["kind"] == json!("object")
    }));
}

#[test]
fn sdk_release_b_filtered_list_cursor_does_not_stall_on_no_matches() {
    let daemon = RpcDaemon::test_instance();
    let topic_a = daemon
        .handle_rpc(rpc_request(110, "sdk_topic_create_v2", json!({ "topic_path": "ops/a" })))
        .expect("topic a");
    let topic_b = daemon
        .handle_rpc(rpc_request(111, "sdk_topic_create_v2", json!({ "topic_path": "ops/b" })))
        .expect("topic b");
    let topic_a_id = topic_a.result.expect("result")["topic"]["topic_id"]
        .as_str()
        .expect("topic_a_id")
        .to_string();
    let topic_b_id = topic_b.result.expect("result")["topic"]["topic_id"]
        .as_str()
        .expect("topic_b_id")
        .to_string();

    let _ = daemon
        .handle_rpc(rpc_request(
            112,
            "sdk_attachment_store_v2",
            json!({
                "name": "a.bin",
                "content_type": "application/octet-stream",
                "bytes_base64": "AA==",
                "topic_ids": [topic_a_id.clone()]
            }),
        ))
        .expect("attachment store");
    let _ = daemon
        .handle_rpc(rpc_request(
            113,
            "sdk_marker_create_v2",
            json!({
                "label": "A",
                "position": { "lat": 1.0, "lon": 1.0, "alt_m": null },
                "topic_id": topic_a_id
            }),
        ))
        .expect("marker create");

    let attachment_list = daemon
        .handle_rpc(rpc_request(
            114,
            "sdk_attachment_list_v2",
            json!({ "topic_id": topic_b_id.clone(), "cursor": null, "limit": 10 }),
        ))
        .expect("attachment list");
    assert!(attachment_list.error.is_none());
    let attachment_result = attachment_list.result.expect("attachment list result");
    assert_eq!(attachment_result["attachments"], json!([]));
    assert_eq!(attachment_result["next_cursor"], JsonValue::Null);

    let marker_list = daemon
        .handle_rpc(rpc_request(
            115,
            "sdk_marker_list_v2",
            json!({ "topic_id": topic_b_id, "cursor": null, "limit": 10 }),
        ))
        .expect("marker list");
    assert!(marker_list.error.is_none());
    let marker_result = marker_list.result.expect("marker list result");
    assert_eq!(marker_result["markers"], json!([]));
    assert_eq!(marker_result["next_cursor"], JsonValue::Null);
}
