#[test]
fn sdk_poll_events_v2_validates_cursor_and_expires_stale_tokens() {
    let daemon = RpcDaemon::test_instance();
    daemon.emit_event(RpcEvent {
        event_type: "inbound".to_string(),
        payload: json!({ "message_id": "m-1" }),
    });
    let first = daemon
        .handle_rpc(rpc_request(
            3,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 4
            }),
        ))
        .expect("poll");
    let first_result = first.result.expect("result");
    let cursor = first_result["next_cursor"].as_str().expect("cursor").to_string();
    assert!(first_result["events"].as_array().is_some_and(|events| !events.is_empty()));

    let invalid = daemon
        .handle_rpc(rpc_request(
            4,
            "sdk_poll_events_v2",
            json!({
                "cursor": "bad-cursor",
                "max": 4
            }),
        ))
        .expect("invalid poll should still return response");
    assert_eq!(invalid.error.expect("error").code, "SDK_RUNTIME_INVALID_CURSOR");

    for idx in 0..(SDK_EVENT_LOG_CAPACITY + 8) {
        daemon.emit_event(RpcEvent {
            event_type: "inbound".to_string(),
            payload: json!({ "message_id": format!("overflow-{idx}") }),
        });
    }

    let expired = daemon
        .handle_rpc(rpc_request(
            5,
            "sdk_poll_events_v2",
            json!({
                "cursor": cursor,
                "max": 2
            }),
        ))
        .expect("expired poll should return response");
    assert_eq!(expired.error.expect("error").code, "SDK_RUNTIME_CURSOR_EXPIRED");
}

#[test]
fn sdk_poll_events_v2_requires_successful_reset_after_degraded_state() {
    let daemon = RpcDaemon::test_instance();
    daemon.emit_event(RpcEvent { event_type: "inbound".to_string(), payload: json!({}) });
    let first = daemon
        .handle_rpc(rpc_request(
            30,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 1
            }),
        ))
        .expect("initial poll");
    let cursor = first.result.expect("result")["next_cursor"].as_str().expect("cursor").to_string();

    for idx in 0..(SDK_EVENT_LOG_CAPACITY + 4) {
        daemon.emit_event(RpcEvent {
            event_type: "inbound".to_string(),
            payload: json!({ "idx": idx }),
        });
    }

    let expired = daemon
        .handle_rpc(rpc_request(
            31,
            "sdk_poll_events_v2",
            json!({
                "cursor": cursor,
                "max": 1
            }),
        ))
        .expect("expired");
    assert_eq!(expired.error.expect("error").code, "SDK_RUNTIME_CURSOR_EXPIRED");

    let invalid_reset = daemon
        .handle_rpc(rpc_request(
            32,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 0
            }),
        ))
        .expect("invalid reset");
    assert_eq!(invalid_reset.error.expect("error").code, "SDK_VALIDATION_INVALID_ARGUMENT");

    let still_degraded = daemon
        .handle_rpc(rpc_request(
            33,
            "sdk_poll_events_v2",
            json!({
                "cursor": "v2:test-identity:sdk-events:999999",
                "max": 1
            }),
        ))
        .expect("still degraded");
    assert_eq!(still_degraded.error.expect("error").code, "SDK_RUNTIME_STREAM_DEGRADED");

    let reset_ok = daemon
        .handle_rpc(rpc_request(
            34,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 1
            }),
        ))
        .expect("reset");
    assert!(reset_ok.error.is_none());
}

#[test]
fn sdk_send_v2_persists_outbound_message() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(rpc_request(
            5,
            "sdk_send_v2",
            json!({
                "id": "sdk-send-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("sdk_send_v2");
    assert!(response.error.is_none());
    assert_eq!(response.result.expect("result")["message_id"], json!("sdk-send-1"));
}

#[test]
fn sdk_send_batch_v2_persists_ordered_outbound_messages() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(rpc_request(
            55,
            "sdk_send_batch_v2",
            json!({
                "batch_id": "batch-1",
                "source": "src",
                "messages": [
                    {
                        "id": "batch-msg-1",
                        "destination": "dst-1",
                        "title": "",
                        "content": "hello one"
                    },
                    {
                        "id": "batch-msg-2",
                        "destination": "dst-2",
                        "title": "RCH",
                        "content": "hello two",
                        "method": "propagated"
                    }
                ]
            }),
        ))
        .expect("sdk_send_batch_v2");
    assert!(response.error.is_none());
    let result = response.result.expect("result");
    assert_eq!(result["batch_id"], json!("batch-1"));
    assert_eq!(result["accepted_count"], json!(2));
    assert_eq!(result["rejected_count"], json!(0));
    assert_eq!(result["results"][0]["id"], json!("batch-msg-1"));
    assert_eq!(result["results"][0]["message_id"], json!("batch-msg-1"));
    assert_eq!(result["results"][0]["accepted"], json!(true));
    assert_eq!(result["results"][1]["id"], json!("batch-msg-2"));
    assert_eq!(result["results"][1]["message_id"], json!("batch-msg-2"));
    assert_eq!(result["results"][1]["accepted"], json!(true));

    let messages = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_messages".into(), params: None })
        .expect("list messages")
        .result
        .expect("result");
    let rows = messages["messages"].as_array().expect("messages");
    assert!(rows.iter().any(|row| row["id"] == json!("batch-msg-1")));
    assert!(rows.iter().any(|row| row["id"] == json!("batch-msg-2")));
}

#[test]
fn sdk_send_v2_exposes_stage_metrics() {
    let daemon = RpcDaemon::test_instance();
    let baseline = daemon.metrics_snapshot();

    let response = daemon
        .handle_rpc(rpc_request(
            6,
            "sdk_send_v2",
            json!({
                "id": "sdk-send-metrics-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("sdk_send_v2");
    assert!(response.error.is_none());

    let snapshot = daemon.metrics_snapshot();
    let before = &baseline["counters"];
    let after = &snapshot["counters"];
    assert_eq!(
        after["sdk_send_store_write_ops_total"].as_u64(),
        Some(before["sdk_send_store_write_ops_total"].as_u64().unwrap_or(0).saturating_add(1))
    );
    assert_eq!(
        after["sdk_send_delivery_schedule_ops_total"].as_u64(),
        Some(
            before["sdk_send_delivery_schedule_ops_total"].as_u64().unwrap_or(0).saturating_add(1)
        )
    );
    assert_eq!(
        after["sdk_send_event_publish_ops_total"].as_u64(),
        Some(before["sdk_send_event_publish_ops_total"].as_u64().unwrap_or(0).saturating_add(1))
    );
    assert!(
        after["sdk_send_store_write_ns_total"].as_u64().unwrap_or(0)
            >= before["sdk_send_store_write_ns_total"].as_u64().unwrap_or(0)
    );
    assert!(
        after["sdk_send_delivery_schedule_ns_total"].as_u64().unwrap_or(0)
            >= before["sdk_send_delivery_schedule_ns_total"].as_u64().unwrap_or(0)
    );
    assert!(
        after["sdk_send_event_publish_ns_total"].as_u64().unwrap_or(0)
            >= before["sdk_send_event_publish_ns_total"].as_u64().unwrap_or(0)
    );
}

#[test]
fn sdk_poll_events_v2_rejects_oversized_event_payload() {
    let daemon = RpcDaemon::test_instance();
    let configure = daemon
        .handle_rpc(rpc_request(
            40,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_event_bytes": 16_384 } }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    let first_poll = daemon
        .handle_rpc(rpc_request(
            41,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 8
            }),
        ))
        .expect("poll");
    let cursor = first_poll.result.expect("result")["next_cursor"].as_str().map(ToOwned::to_owned);

    daemon.emit_event(RpcEvent {
        event_type: "inbound".to_string(),
        payload: json!({
            "message_id": "too-large",
            "blob": "x".repeat(17_000),
        }),
    });

    let response = daemon
        .handle_rpc(rpc_request(
            42,
            "sdk_poll_events_v2",
            json!({
                "cursor": cursor,
                "max": 1
            }),
        ))
        .expect("poll");
    assert_eq!(response.error.expect("error").code, "SDK_VALIDATION_EVENT_TOO_LARGE");
}

#[test]
fn sdk_poll_events_v2_rejects_oversized_batch() {
    let daemon = RpcDaemon::test_instance();
    let configure = daemon
        .handle_rpc(rpc_request(
            50,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_stream": {
                        "max_event_bytes": 900,
                        "max_batch_bytes": 1_024
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    let first_poll = daemon
        .handle_rpc(rpc_request(
            51,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 8
            }),
        ))
        .expect("poll");
    let cursor = first_poll.result.expect("result")["next_cursor"].as_str().map(ToOwned::to_owned);

    daemon.emit_event(RpcEvent {
        event_type: "inbound".to_string(),
        payload: json!({
            "message_id": "m-batch-1",
            "blob": "x".repeat(768),
        }),
    });
    daemon.emit_event(RpcEvent {
        event_type: "inbound".to_string(),
        payload: json!({
            "message_id": "m-batch-2",
            "blob": "y".repeat(768),
        }),
    });

    let response = daemon
        .handle_rpc(rpc_request(
            52,
            "sdk_poll_events_v2",
            json!({
                "cursor": cursor,
                "max": 2
            }),
        ))
        .expect("poll");
    assert_eq!(response.error.expect("error").code, "SDK_VALIDATION_BATCH_TOO_LARGE");
}

#[test]
fn sdk_poll_events_v2_rejects_event_with_too_many_extension_keys() {
    let daemon = RpcDaemon::test_instance();
    let configure = daemon
        .handle_rpc(rpc_request(
            60,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_extension_keys": 1 } }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    let first_poll = daemon
        .handle_rpc(rpc_request(
            61,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 8
            }),
        ))
        .expect("poll");
    let cursor = first_poll.result.expect("result")["next_cursor"].as_str().map(ToOwned::to_owned);

    daemon.emit_event(RpcEvent {
        event_type: "inbound".to_string(),
        payload: json!({
            "message_id": "m-ext",
            "extensions": {
                "k1": true,
                "k2": false
            }
        }),
    });

    let response = daemon
        .handle_rpc(rpc_request(
            62,
            "sdk_poll_events_v2",
            json!({
                "cursor": cursor,
                "max": 1
            }),
        ))
        .expect("poll");
    assert_eq!(response.error.expect("error").code, "SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED");
}

#[test]
fn sdk_domain_methods_respect_capability_gating_when_removed() {
    let daemon = RpcDaemon::test_instance();
    {
        let mut capabilities = daemon
            .sdk_effective_capabilities
            .lock()
            .expect("sdk_effective_capabilities mutex poisoned");
        *capabilities = vec!["sdk.capability.cursor_replay".to_string()];
    }
    let response = daemon
        .handle_rpc(rpc_request(77, "sdk_topic_create_v2", json!({ "topic_path": "ops/alpha" })))
        .expect("rpc response");
    let error = response.error.expect("expected capability error");
    assert_eq!(error.code, "SDK_CAPABILITY_DISABLED");
    assert!(error.message.contains("sdk_topic_create_v2"));
}
