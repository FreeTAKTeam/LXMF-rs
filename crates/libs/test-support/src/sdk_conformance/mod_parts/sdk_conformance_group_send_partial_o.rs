#[test]
fn sdk_conformance_group_send_partial_outcomes() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let result = client
        .send_group(GroupSendRequest::new(
            "source.test",
            vec!["destination.test", "", "destination.test"],
            json!({ "content": "group payload" }),
        ))
        .expect("group send should return outcomes");

    assert_eq!(result.outcomes.len(), 3);
    assert_eq!(
        result.accepted_count + result.deferred_count + result.failed_count,
        result.outcomes.len(),
        "group send counters must match number of outcomes"
    );
    assert!(
        result.outcomes.iter().any(
            |outcome| outcome.reason_code.as_deref() == Some("SDK_VALIDATION_INVALID_ARGUMENT")
        ),
        "group send should classify empty destinations as per-recipient validation failures"
    );
}

#[test]
fn sdk_conformance_poll_cursor_monotonicity_and_invalid_cursor() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    harness.emit_event("health_snapshot", json!({ "status": "ok", "idx": 1 }));
    harness.emit_event("health_snapshot", json!({ "status": "ok", "idx": 2 }));

    let first = client.poll_events(None, 1).expect("first poll");
    assert_eq!(first.events.len(), 1);
    let first_seq = first.events[0].seq_no;
    let second =
        client.poll_events(Some(first.next_cursor.clone()), 1).expect("second poll with cursor");
    assert_eq!(second.events.len(), 1);
    assert!(second.events[0].seq_no > first_seq);

    let err = client
        .poll_events(Some(EventCursor("invalid-cursor-token".to_owned())), 1)
        .expect_err("invalid cursor must fail");
    assert_eq!(err.machine_code, "SDK_RUNTIME_INVALID_CURSOR");
}

#[test]
fn sdk_conformance_expired_cursor_requires_reset_and_reports_gap() {
    let harness = RpcHarness::new();
    let client = harness.client();
    let mut start_request = base_start_request();
    start_request.config.overflow_policy = OverflowPolicy::DropOldest;
    client.start(start_request).expect("start");

    harness.emit_event("seed_event", json!({ "idx": 1 }));
    let first = client.poll_events(None, 1).expect("initial poll");
    assert_eq!(first.events.len(), 1);
    let stale_cursor = first.next_cursor;

    for idx in 0..EVENT_LOG_OVERFLOW_TRIGGER {
        harness.emit_event("overflow_event", json!({ "idx": idx }));
    }

    let expired = client
        .poll_events(Some(stale_cursor), 1)
        .expect_err("stale cursor outside retained window must expire");
    assert_eq!(expired.machine_code, "SDK_RUNTIME_CURSOR_EXPIRED");

    let degraded = client
        .poll_events(Some(EventCursor("v2:sdk-test-runtime:sdk-events-v2:999999".to_owned())), 1)
        .expect_err("cursored poll after expiry must remain degraded until reset");
    assert_eq!(degraded.machine_code, "SDK_RUNTIME_STREAM_DEGRADED");

    let reset = client.poll_events(None, 8).expect("explicit reset");
    assert!(
        reset.events.iter().any(|event| event.event_type == "StreamGap"),
        "reset after cursor expiry must surface a StreamGap event"
    );
    assert!(reset.dropped_count > 0, "reset should report dropped events");
}

#[test]
fn sdk_conformance_stream_gap_is_emitted_after_log_overflow() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    for idx in 0..EVENT_LOG_OVERFLOW_TRIGGER {
        harness.emit_event("flood", json!({ "idx": idx }));
    }

    let batch = client.poll_events(None, 8).expect("poll with overflow");
    assert!(!batch.events.is_empty(), "batch should include stream gap event");
    assert!(
        batch.events.iter().any(|event| event.event_type == "StreamGap"),
        "batch should contain StreamGap"
    );
    assert!(batch.dropped_count > 0, "dropped_count should report overflow");
}

#[test]
fn sdk_conformance_subscribe_events_tail_starts_from_current_end() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    harness.emit_event("seed_event", json!({ "idx": 1 }));
    harness.emit_event("seed_event", json!({ "idx": 2 }));

    let subscription =
        client.subscribe_events(SubscriptionStart::Tail).expect("subscribe with tail start");
    let first =
        client.poll_events(subscription.cursor.clone(), 16).expect("poll using tail cursor");
    assert!(
        first.events.iter().all(|event| event.event_type != "seed_event"),
        "tail subscription should skip backlog events"
    );

    harness.emit_event("live_event", json!({ "idx": 3 }));
    let second = client.poll_events(Some(first.next_cursor.clone()), 16).expect("poll live events");
    assert!(
        second.events.iter().any(|event| event.event_type == "live_event"),
        "tail cursor should include events emitted after subscription"
    );
}

#[test]
fn sdk_conformance_duplicate_delivery_replay_preserves_event_identity() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let subscription =
        client.subscribe_events(SubscriptionStart::Head).expect("subscribe from head");
    harness.emit_event("duplicate_probe", json!({ "idx": 1 }));

    let first = client.poll_events(subscription.cursor.clone(), 1).expect("first delivery");
    let replay = client.poll_events(subscription.cursor, 1).expect("replayed delivery");

    assert_eq!(first.events.len(), 1);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(
        first.events[0].event_id, replay.events[0].event_id,
        "at-least-once replay must preserve the event identity for consumer dedupe"
    );
    assert_eq!(
        first.events[0].seq_no, replay.events[0].seq_no,
        "at-least-once replay must preserve event ordering metadata"
    );
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_catches_up_after_stream_close() {
    use tokio_stream::StreamExt;

    let backend = AppStreamConformanceBackend::new(
        vec![sdk_event(1, "live_probe"), sdk_event(2, "live_probe")],
        vec![sdk_event(3, "catchup_probe")],
    );
    let state = Arc::clone(&backend.state);
    let app = lxmf_sdk::app::Client::new(backend);
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    let first = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("first event should arrive")
        .expect("stream should remain open")
        .expect("first event should decode");
    let second = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("second event should arrive")
        .expect("stream should remain open")
        .expect("second event should decode");
    let catchup = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("catch-up event should arrive")
        .expect("stream should remain open")
        .expect("catch-up event should decode");

    assert_eq!((first.metadata.seq_no, second.metadata.seq_no, catchup.metadata.seq_no), (1, 2, 3));
    let cursors = state.poll_cursors.lock().expect("poll cursors mutex");
    assert_eq!(
        cursors.first().and_then(|cursor| cursor.as_ref()).map(|cursor| cursor.0.as_str()),
        Some("v2:conformance-runtime:sdk-events-v2:2"),
        "catch-up poll must resume from the last delivered native event cursor"
    );
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_backpressures_slow_consumers() {
    use tokio_stream::StreamExt;

    let stats = Arc::new(SlowConsumerStats {
        attempted_sends: AtomicUsize::new(0),
        completed_sends: AtomicUsize::new(0),
    });
    let backend = SlowConsumerConformanceBackend::new(3, Arc::clone(&stats));
    let app = lxmf_sdk::app::Client::new(backend);
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        stats.completed_sends.load(Ordering::Relaxed) <= 1,
        "bounded stream should not complete all sends before the consumer starts draining"
    );
    assert!(
        stats.attempted_sends.load(Ordering::Relaxed) >= 2,
        "producer should be blocked on a later send, not idle"
    );

    let first = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("first event should arrive")
        .expect("stream should remain open")
        .expect("first event should decode");
    assert_eq!(first.metadata.seq_no, 1);

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        stats.completed_sends.load(Ordering::Relaxed) >= 2,
        "draining one event should release producer progress"
    );
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_delivers_ordered_typed_events() {
    use tokio_stream::StreamExt;

    let harness = RpcHarness::new();
    let app = lxmf_sdk::app::Client::rpc(harness.endpoint.clone());
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    harness.emit_event("conformance_event", json!({ "idx": 1 }));
    harness.emit_event("conformance_event", json!({ "idx": 2 }));

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    let mut observed = Vec::new();
    while observed.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(2), events.next())
            .await
            .expect("event stream should make progress")
            .expect("event stream should remain open")
            .expect("event should decode");
        if event.raw_event_type == "conformance_event" {
            observed.push((
                event.metadata.seq_no,
                event.details.get("idx").and_then(JsonValue::as_u64).expect("idx"),
            ));
        }
    }

    assert_eq!(observed[0].1, 1);
    assert_eq!(observed[1].1, 2);
    assert!(observed[1].0 > observed[0].0, "app event stream must preserve SDK event ordering");
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_reports_gap_as_typed_event() {
    use tokio_stream::StreamExt;

    let harness = RpcHarness::new();
    let app = lxmf_sdk::app::Client::rpc(harness.endpoint.clone());
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    for idx in 0..EVENT_LOG_OVERFLOW_TRIGGER {
        harness.emit_event("flood", json!({ "idx": idx }));
    }

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    let event = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .expect("event stream should report overflow")
        .expect("event stream should remain open")
        .expect("event should decode");

    assert!(
        matches!(event.kind, lxmf_sdk::app::EventKind::StreamGapDetected(_)),
        "app event stream must surface stream gaps as typed events"
    );
    let status = app.runtime().status().expect("runtime status");
    assert_eq!(status.state, lxmf_sdk::app::RunState::Degraded);
}

#[test]
fn sdk_conformance_cancel_accepted_and_too_late_paths() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let pending_message_id = "pending-cancel-message";
    let receive_response = harness.rpc_call(
        "receive_message",
        Some(json!({
            "id": pending_message_id,
            "source": "source.test",
            "destination": "destination.test",
            "title": "",
            "content": "inbound message for cancel test",
            "fields": null
        })),
    );
    assert!(receive_response.error.is_none(), "receive_message should succeed");

    let cancel_result = client.cancel(MessageId(pending_message_id.to_owned())).expect("cancel");
    assert_eq!(cancel_result, CancelResult::Accepted);

    let sent_id = client.send(send_request("already-sent", None)).expect("send");
    let sent_id_raw = sent_id.0.clone();
    let receipt_response = harness.rpc_call(
        "record_receipt",
        Some(json!({
            "message_id": sent_id_raw,
            "status": "sent",
        })),
    );
    assert!(receipt_response.error.is_none(), "record_receipt should succeed");
    let too_late = client.cancel(sent_id).expect("cancel too late path");
    assert_eq!(too_late, CancelResult::TooLateToCancel);
}

#[tokio::test]
async fn sdk_conformance_unknown_message_status_and_cancel() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let unknown = MessageId("unknown-conformance-message".to_owned());
    assert!(
        client.status(unknown.clone()).expect("sync status").is_none(),
        "unknown sync status must return no delivery snapshot"
    );
    assert!(
        client.status_async(unknown.clone()).await.expect("async status").is_none(),
        "unknown async status must return no delivery snapshot"
    );
    assert_eq!(
        client.cancel(unknown).expect("cancel unknown message"),
        CancelResult::NotFound,
        "unknown cancel must return the typed equivalent of false"
    );
}

#[test]
fn sdk_conformance_configure_cas_conflict() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let first = client.configure(0, overflow_patch()).expect("first configure");
    assert!(first.accepted);
    assert_eq!(first.revision, Some(1));

    let err = client.configure(0, overflow_patch()).expect_err("stale revision must fail");
    assert_eq!(err.machine_code, "SDK_CONFIG_CONFLICT");
}

#[test]
fn sdk_conformance_snapshot_tracks_event_position() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    harness.emit_event("policy_changed", json!({ "scope": "delivery" }));

    let snapshot = client.snapshot().expect("snapshot");
    assert_eq!(snapshot.active_contract_version, 2);
    assert!(snapshot.event_stream_position > 0);
}

#[test]
fn sdk_conformance_poll_rejects_max_over_limit() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let err = client.poll_events(None, 257).expect_err("poll max above negotiated limit must fail");
    assert_eq!(err.machine_code, "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED");
}

#[test]
fn sdk_conformance_sent_terminality_depends_on_receipt_capability() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let message_id = client.send(send_request("terminality", None)).expect("send");
    let message_id_raw = message_id.0.clone();
    let response = harness.rpc_call(
        "record_receipt",
        Some(json!({
            "message_id": message_id_raw,
            "status": "sent",
        })),
    );
    assert!(response.error.is_none(), "record_receipt should succeed");
    let snapshot = client
        .status(MessageId(message_id.0.clone()))
        .expect("status")
        .expect("message should exist");
    assert!(!snapshot.terminal, "sent must be non-terminal with receipt_terminality");
}
