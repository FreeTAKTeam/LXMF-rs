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
            .handle_rpc(rpc_request(
                91,
                "sdk_topic_get_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
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
        assert_eq!(
            attachment_get.result.expect("result")["attachment"]["name"],
            json!("sample.txt")
        );

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
        let marker_id = marker.result.expect("result")["marker"]["marker_id"]
            .as_str()
            .expect("marker id")
            .to_string();

        let marker_update = daemon
            .handle_rpc(rpc_request(
                100,
                "sdk_marker_update_position_v2",
                json!({
                    "marker_id": marker_id,
                    "position": { "lat": 36.0, "lon": -116.0, "alt_m": null }
                }),
            ))
            .expect("marker update");
        assert!(marker_update.error.is_none());
        assert_eq!(marker_update.result.expect("result")["marker"]["position"]["lat"], json!(36.0));
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

    #[test]
    fn sdk_release_c_domain_methods_roundtrip() {
        let daemon = RpcDaemon::test_instance();
        let list_before =
            daemon.handle_rpc(rpc_request(120, "sdk_identity_list_v2", json!({}))).expect("list");
        assert!(list_before.error.is_none());
        assert!(!list_before.result.expect("result")["identities"]
            .as_array()
            .expect("identity array")
            .is_empty());

        let identity_bundle = json!({
            "identity": "node-b",
            "public_key": "node-b-pub",
            "display_name": "Node B",
            "capabilities": ["ops"],
            "extensions": {}
        });
        let identity_import = daemon
            .handle_rpc(rpc_request(
                121,
                "sdk_identity_import_v2",
                json!({
                    "bundle_base64": BASE64_STANDARD.encode(identity_bundle.to_string().as_bytes()),
                    "passphrase": null
                }),
            ))
            .expect("identity import");
        assert!(identity_import.error.is_none());
        assert_eq!(
            identity_import.result.expect("result")["identity"]["identity"],
            json!("node-b")
        );

        let identity_resolve = daemon
            .handle_rpc(rpc_request(
                122,
                "sdk_identity_resolve_v2",
                json!({ "hash": "node-b-pub" }),
            ))
            .expect("identity resolve");
        assert!(identity_resolve.error.is_none());
        assert_eq!(identity_resolve.result.expect("result")["identity"], json!("node-b"));

        let identity_export = daemon
            .handle_rpc(rpc_request(123, "sdk_identity_export_v2", json!({ "identity": "node-b" })))
            .expect("identity export");
        assert!(identity_export.error.is_none());
        assert!(!identity_export.result.expect("result")["bundle"]["bundle_base64"]
            .as_str()
            .expect("export bundle")
            .is_empty());

        let _ = daemon
            .handle_rpc(rpc_request(
                124,
                "send_message_v2",
                json!({
                    "id": "paper-msg-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "paper body"
                }),
            ))
            .expect("send message for paper");
        let paper_encode = daemon
            .handle_rpc(rpc_request(
                125,
                "sdk_paper_encode_v2",
                json!({ "message_id": "paper-msg-1" }),
            ))
            .expect("paper encode");
        assert!(paper_encode.error.is_none());
        let uri = paper_encode.result.expect("result")["envelope"]["uri"]
            .as_str()
            .expect("paper uri")
            .to_string();
        assert!(uri.starts_with("lxm://"));

        let paper_decode = daemon
            .handle_rpc(rpc_request(126, "sdk_paper_decode_v2", json!({ "uri": uri })))
            .expect("paper decode");
        assert!(paper_decode.error.is_none());
        assert_eq!(paper_decode.result.expect("result")["accepted"], json!(true));

        let command = daemon
            .handle_rpc(rpc_request(
                127,
                "sdk_command_invoke_v2",
                json!({
                    "command": "ping",
                    "target": "node-b",
                    "payload": { "body": "hello" },
                    "timeout_ms": 1000
                }),
            ))
            .expect("command invoke");
        assert!(command.error.is_none());
        let correlation_id = command.result.expect("result")["response"]["payload"]
            ["correlation_id"]
            .as_str()
            .expect("correlation id")
            .to_string();

        let command_reply = daemon
            .handle_rpc(rpc_request(
                128,
                "sdk_command_reply_v2",
                json!({
                    "correlation_id": correlation_id,
                    "accepted": true,
                    "payload": { "reply": "pong" }
                }),
            ))
            .expect("command reply");
        assert!(command_reply.error.is_none());
        assert_eq!(command_reply.result.expect("result")["accepted"], json!(true));

        let voice_open = daemon
            .handle_rpc(rpc_request(
                129,
                "sdk_voice_session_open_v2",
                json!({ "peer_id": "node-b", "codec_hint": "opus" }),
            ))
            .expect("voice open");
        assert!(voice_open.error.is_none());
        let session_id = voice_open.result.expect("result")["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let voice_update = daemon
            .handle_rpc(rpc_request(
                130,
                "sdk_voice_session_update_v2",
                json!({ "session_id": session_id.clone(), "state": "active" }),
            ))
            .expect("voice update");
        assert!(voice_update.error.is_none());
        assert_eq!(voice_update.result.expect("result")["state"], json!("active"));

        let voice_close = daemon
            .handle_rpc(rpc_request(
                131,
                "sdk_voice_session_close_v2",
                json!({ "session_id": session_id }),
            ))
            .expect("voice close");
        assert!(voice_close.error.is_none());
        assert_eq!(voice_close.result.expect("result")["accepted"], json!(true));
    }

    #[test]
    fn sdk_domain_state_survives_restart() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let run_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos();
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
                .handle_rpc(rpc_request(200, "sdk_topic_create_v2", json!({ "topic_path": "ops/persist" })))
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
            attachment_id = attachment.result.expect("attachment result")["attachment"]["attachment_id"]
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
            assert_eq!(
                attachment_download.result.expect("result")["bytes_base64"],
                json!("AQID")
            );

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

            let voice_close = daemon
                .handle_rpc(rpc_request(
                    216,
                    "sdk_voice_session_close_v2",
                    json!({ "session_id": session_id.clone() }),
                ))
                .expect("voice close after restart");
            assert!(voice_close.error.is_none());

            let topic_2 = daemon
                .handle_rpc(rpc_request(217, "sdk_topic_create_v2", json!({ "topic_path": "ops/persist-2" })))
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

        let run_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos();
        let db_path = std::env::temp_dir()
            .join(format!("lxmf-rs-sdk-authority-{run_id}-{}.sqlite", std::process::id()));

        let store_a = MessagesStore::open(db_path.as_path()).expect("open sqlite store A");
        let daemon_a = RpcDaemon::with_store(store_a, "authority-node".to_string());
        let store_b = MessagesStore::open(db_path.as_path()).expect("open sqlite store B");
        let daemon_b = RpcDaemon::with_store(store_b, "authority-node".to_string());

        let topic = daemon_a
            .handle_rpc(rpc_request(
                300,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/shared" }),
            ))
            .expect("topic create");
        assert!(topic.error.is_none());
        let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
            .as_str()
            .expect("topic id")
            .to_string();

        let topic_get_from_b = daemon_b
            .handle_rpc(rpc_request(
                301,
                "sdk_topic_get_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
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
            .handle_rpc(rpc_request(
                303,
                "sdk_marker_list_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
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

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn sdk_config_and_terminal_state_survive_restart_without_orphan_transitions() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let run_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos();
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
                .handle_rpc(rpc_request(
                    404,
                    "sdk_snapshot_v2",
                    json!({ "include_counts": true }),
                ))
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
                .handle_rpc(rpc_request(
                    406,
                    "sdk_topic_get_v2",
                    json!({ "topic_id": topic_id }),
                ))
                .expect("topic get");
            assert!(topic_get.error.is_none());

            let status = daemon
                .handle_rpc(rpc_request(
                    407,
                    "sdk_status_v2",
                    json!({ "message_id": message_id }),
                ))
                .expect("status");
            assert!(status.error.is_none());
            assert_eq!(
                status.result.expect("result")["message"]["receipt_status"],
                json!("cancelled")
            );

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
