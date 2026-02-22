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
        let cursor =
            first.result.expect("result")["next_cursor"].as_str().expect("cursor").to_string();

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
        let cursor =
            first_poll.result.expect("result")["next_cursor"].as_str().map(ToOwned::to_owned);

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
        let cursor =
            first_poll.result.expect("result")["next_cursor"].as_str().map(ToOwned::to_owned);

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
        let cursor =
            first_poll.result.expect("result")["next_cursor"].as_str().map(ToOwned::to_owned);

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
        assert_eq!(
            response.error.expect("error").code,
            "SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED"
        );
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
            .handle_rpc(rpc_request(
                77,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/alpha" }),
            ))
            .expect("rpc response");
        let error = response.error.expect("expected capability error");
        assert_eq!(error.code, "SDK_CAPABILITY_DISABLED");
        assert!(error.message.contains("sdk_topic_create_v2"));
    }

    #[test]
    fn sdk_overflow_policy_reject_keeps_oldest_events_and_drops_newest() {
        let daemon = RpcDaemon::test_instance();
        let configure = daemon
            .handle_rpc(rpc_request(
                90,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "overflow_policy": "reject",
                        "event_stream": { "max_poll_events": 2048 }
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none());

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 1) {
            daemon.emit_event(RpcEvent {
                event_type: "inbound".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let response = daemon
            .handle_rpc(rpc_request(
                91,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 2048
                }),
            ))
            .expect("poll");
        let result = response.result.expect("result");
        let events = result["events"].as_array().expect("events array");
        let payload_indices = events
            .iter()
            .filter_map(|row| {
                row.get("payload")
                    .and_then(|payload| payload.get("idx"))
                    .and_then(JsonValue::as_u64)
            })
            .collect::<Vec<_>>();

        assert!(result["dropped_count"].as_u64().unwrap_or(0) > 0);
        assert!(
            payload_indices.contains(&0),
            "reject policy should retain oldest entries instead of evicting head"
        );
        assert!(
            !payload_indices.contains(&(SDK_EVENT_LOG_CAPACITY as u64)),
            "reject policy should drop newest event when capacity is exhausted"
        );
    }

    #[test]
    fn sdk_overflow_policy_drop_oldest_evicts_head_entries() {
        let daemon = RpcDaemon::test_instance();
        let configure = daemon
            .handle_rpc(rpc_request(
                92,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "overflow_policy": "drop_oldest",
                        "event_stream": { "max_poll_events": 2048 }
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none());

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 1) {
            daemon.emit_event(RpcEvent {
                event_type: "inbound".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let response = daemon
            .handle_rpc(rpc_request(
                93,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 2048
                }),
            ))
            .expect("poll");
        let result = response.result.expect("result");
        let events = result["events"].as_array().expect("events array");
        let payload_indices = events
            .iter()
            .filter_map(|row| {
                row.get("payload")
                    .and_then(|payload| payload.get("idx"))
                    .and_then(JsonValue::as_u64)
            })
            .collect::<Vec<_>>();

        assert!(result["dropped_count"].as_u64().unwrap_or(0) > 0);
        assert!(
            !payload_indices.contains(&0),
            "drop_oldest policy should evict oldest entry once capacity is exceeded"
        );
        assert!(
            payload_indices.contains(&(SDK_EVENT_LOG_CAPACITY as u64)),
            "drop_oldest policy should retain newest event"
        );
    }

    #[test]
    fn sdk_event_queues_remain_bounded_under_sustained_load() {
        let daemon = RpcDaemon::test_instance();
        let configure = daemon
            .handle_rpc(rpc_request(
                94,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "overflow_policy": "drop_oldest",
                        "event_stream": { "max_poll_events": 4096 }
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none());

        for idx in 0..(SDK_EVENT_LOG_CAPACITY * 8) {
            daemon.emit_event(RpcEvent {
                event_type: "queue_pressure".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let legacy_len = daemon.event_queue.lock().expect("event_queue mutex poisoned").len();
        let sdk_len = daemon.sdk_event_log.lock().expect("sdk_event_log mutex poisoned").len();
        let dropped = *daemon
            .sdk_dropped_event_count
            .lock()
            .expect("sdk_dropped_event_count mutex poisoned");

        assert!(
            legacy_len <= LEGACY_EVENT_QUEUE_CAPACITY,
            "legacy queue must stay bounded under load"
        );
        assert_eq!(
            sdk_len, SDK_EVENT_LOG_CAPACITY,
            "sdk event log must remain capped under load"
        );
        assert!(dropped > 0, "drop_oldest policy should report dropped events under pressure");
    }

    #[test]
    fn sdk_property_cursor_monotonicity_randomized_poll_batches() {
        let daemon = RpcDaemon::test_instance();
        let total_events = 240_u64;
        for idx in 0..total_events {
            daemon.emit_event(RpcEvent {
                event_type: "property_cursor".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let mut cursor: Option<String> = None;
        let mut last_seq = 0_u64;
        let mut seen = std::collections::BTreeSet::new();
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;

        for iteration in 0..512_u64 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let max = ((seed % 11) + 1) as usize;
            let response = daemon
                .handle_rpc(rpc_request(
                    1_000 + iteration,
                    "sdk_poll_events_v2",
                    json!({
                        "cursor": cursor.clone(),
                        "max": max,
                    }),
                ))
                .expect("poll");
            assert!(response.error.is_none(), "poll should remain stable for randomized batches");
            let result = response.result.expect("result");
            let events = result["events"].as_array().expect("events array");
            for event in events {
                let seq = event["seq_no"].as_u64().expect("seq_no");
                assert!(seq > last_seq, "event sequence must remain strictly increasing");
                assert!(seen.insert(seq), "sequence IDs must not repeat");
                last_seq = seq;
            }
            cursor = result["next_cursor"].as_str().map(ToOwned::to_owned);
            if seen.len() >= total_events as usize {
                break;
            }
        }

        assert_eq!(
            seen.len(),
            total_events as usize,
            "randomized cursor polling should read every emitted event exactly once"
        );
    }

    #[test]
    fn sdk_property_stream_gap_reports_consistent_drop_metadata() {
        let daemon = RpcDaemon::test_instance();
        let configure = daemon
            .handle_rpc(rpc_request(
                1_100,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "overflow_policy": "drop_oldest",
                        "event_stream": { "max_poll_events": 4096 }
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none());

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 64) {
            daemon.emit_event(RpcEvent {
                event_type: "property_gap".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let first = daemon
            .handle_rpc(rpc_request(
                1_101,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 32
                }),
            ))
            .expect("first poll");
        assert!(first.error.is_none(), "first poll should succeed");
        let first_result = first.result.expect("result");
        let dropped_count = first_result["dropped_count"].as_u64().unwrap_or(0);
        assert!(dropped_count > 0, "overflow run should report dropped_count");

        let events = first_result["events"].as_array().expect("events array");
        let gap_event = events
            .iter()
            .find(|event| event.get("event_type").and_then(JsonValue::as_str) == Some("StreamGap"))
            .expect("first poll should include StreamGap marker");
        let gap_payload = gap_event["payload"].as_object().expect("gap payload object");
        let expected_seq_no =
            gap_payload.get("expected_seq_no").and_then(JsonValue::as_u64).expect("expected");
        let observed_seq_no =
            gap_payload.get("observed_seq_no").and_then(JsonValue::as_u64).expect("observed");
        let payload_dropped =
            gap_payload.get("dropped_count").and_then(JsonValue::as_u64).expect("dropped");
        assert_eq!(payload_dropped, dropped_count, "gap payload must match top-level dropped_count");
        assert_eq!(
            expected_seq_no.saturating_add(payload_dropped),
            observed_seq_no,
            "gap metadata invariant expected + dropped == observed must hold"
        );

        let mut last_seq = 0_u64;
        for event in events {
            let seq = event["seq_no"].as_u64().expect("seq");
            assert!(seq > last_seq, "first poll sequence should be strictly increasing");
            last_seq = seq;
        }

        let follow_cursor = first_result["next_cursor"].as_str().expect("cursor").to_string();
        let follow = daemon
            .handle_rpc(rpc_request(
                1_102,
                "sdk_poll_events_v2",
                json!({
                    "cursor": follow_cursor,
                    "max": 16
                }),
            ))
            .expect("follow poll");
        assert!(follow.error.is_none(), "follow-up poll should succeed");
        let follow_result = follow.result.expect("result");
        assert_eq!(
            follow_result["dropped_count"].as_u64().unwrap_or(u64::MAX),
            0,
            "cursored polls must not re-report dropped_count"
        );
        assert!(
            follow_result["events"].as_array().is_some_and(|rows| {
                rows.iter().all(|event| {
                    event.get("event_type").and_then(JsonValue::as_str) != Some("StreamGap")
                })
            }),
            "cursored polls must not inject StreamGap events"
        );
    }
