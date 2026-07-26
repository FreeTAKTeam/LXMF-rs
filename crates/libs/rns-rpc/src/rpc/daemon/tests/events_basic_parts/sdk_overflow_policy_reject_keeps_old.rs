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
            row.get("payload").and_then(|payload| payload.get("idx")).and_then(JsonValue::as_u64)
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
            row.get("payload").and_then(|payload| payload.get("idx")).and_then(JsonValue::as_u64)
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
    let dropped =
        *daemon.sdk_dropped_event_count.lock().expect("sdk_dropped_event_count mutex poisoned");

    assert!(legacy_len <= LEGACY_EVENT_QUEUE_CAPACITY, "legacy queue must stay bounded under load");
    assert_eq!(sdk_len, SDK_EVENT_LOG_CAPACITY, "sdk event log must remain capped under load");
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

#[test]
fn sdk_stream_gap_only_batch_keeps_cursor_before_retained_events() {
    let daemon = RpcDaemon::test_instance();
    let configure = daemon
        .handle_rpc(rpc_request(
            1_103,
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

    for idx in 0..(SDK_EVENT_LOG_CAPACITY + 1) {
        daemon.emit_event(RpcEvent {
            event_type: "property_gap_cursor".to_string(),
            payload: json!({ "idx": idx }),
        });
    }

    let first = daemon
        .handle_rpc(rpc_request(
            1_104,
            "sdk_poll_events_v2",
            json!({ "cursor": null, "max": 1 }),
        ))
        .expect("gap-only poll");
    let first_result = first.result.expect("gap-only result");
    let events = first_result["events"].as_array().expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_type"], json!("StreamGap"));

    let follow = daemon
        .handle_rpc(rpc_request(
            1_105,
            "sdk_poll_events_v2",
            json!({
                "cursor": first_result["next_cursor"].as_str().expect("cursor"),
                "max": 1
            }),
        ))
        .expect("retained-event poll");
    let follow_result = follow.result.expect("retained result");
    let follow_events = follow_result["events"].as_array().expect("events");
    assert_eq!(follow_events.len(), 1);
    assert_eq!(follow_events[0]["event_type"], json!("property_gap_cursor"));
}

#[test]
fn native_stream_gap_frame_matches_poll_gap_metadata_shape() {
    let daemon = RpcDaemon::test_instance();
    let frame = daemon.sdk_stream_gap_frame(42, 47, 5);

    assert_eq!(frame["event_type"].as_str(), Some("StreamGap"));
    assert_eq!(frame["seq_no"].as_u64(), Some(46));
    assert_eq!(frame["event_id"].as_str(), Some("gap-46"));
    assert_eq!(frame["payload"]["expected_seq_no"].as_u64(), Some(42));
    assert_eq!(frame["payload"]["observed_seq_no"].as_u64(), Some(47));
    assert_eq!(frame["payload"]["dropped_count"].as_u64(), Some(5));
    assert_eq!(frame["payload"]["recovery_required"].as_bool(), Some(true));
}
