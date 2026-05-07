#[test]
fn discovery_helpers_map_backend_identity_contact_and_presence_models() {
    let app = Client::new(MockBackend::new());

    let identities = app.identities().expect("identities");
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].identity, "alice");
    assert_eq!(identities[0].display_name.as_deref(), Some("Alice"));

    let contacts = app.contacts(Some("cursor-1".to_owned()), Some(5)).expect("contacts");
    assert_eq!(contacts.contacts.len(), 1);
    assert_eq!(contacts.contacts[0].identity, "bob");
    assert_eq!(contacts.contacts[0].trust_level, TrustLevel::Trusted);
    assert_eq!(
        contacts.contacts[0].extensions.get("cursor").and_then(|value| value.as_str()),
        Some("cursor-1")
    );

    let presence = app.presence(None, Some(10)).expect("presence");
    assert_eq!(presence.peers.len(), 2);
    assert_eq!(presence.peers[0].peer_id, "bob");
    assert_eq!(presence.peers[0].display_name.as_deref(), Some("Bob Relay"));
    assert_eq!(presence.peers[0].trust_level, Some(TrustLevel::Trusted));
    assert!(presence.peers[0].bootstrap.unwrap_or(false));
}

#[test]
fn discovery_helpers_update_contacts_and_bootstrap_identities() {
    let app = Client::new(MockBackend::new());

    let updated = app
        .update_contact(
            super::super::discovery::ContactUpdate::new("charlie")
                .with_display_name("Charlie")
                .with_trust_level(TrustLevel::Untrusted)
                .with_bootstrap(true),
        )
        .expect("contact update");
    assert_eq!(updated.identity, "charlie");
    assert_eq!(updated.display_name.as_deref(), Some("Charlie"));
    assert_eq!(updated.trust_level, TrustLevel::Untrusted);
    assert!(updated.bootstrap);

    let bootstrapped = app
        .bootstrap_identity(super::super::discovery::BootstrapRequest::new("delta"))
        .expect("bootstrap");
    assert_eq!(bootstrapped.identity, "delta");
    assert_eq!(bootstrapped.trust_level, TrustLevel::Trusted);
    assert!(bootstrapped.bootstrap);
}

#[test]
fn peer_directory_merges_contact_and_presence_views() {
    let app = Client::new(MockBackend::new());
    let peers = app.peer_directory(Some(10)).expect("peer directory");

    assert_eq!(peers.len(), 2);

    let bob = peers.iter().find(|entry| entry.peer_id == "bob").expect("bob entry");
    assert_eq!(bob.display_name.as_deref(), Some("Bob"));
    assert_eq!(bob.name_source.as_deref(), Some("contact"));
    assert_eq!(bob.trust_level, Some(TrustLevel::Trusted));
    assert!(bob.online);
    assert!(bob.bootstrap);
    assert_eq!(bob.last_seen_ts_ms, Some(200));
    assert_eq!(bob.first_seen_ts_ms, Some(120));
    assert_eq!(bob.seen_count, 3);

    let eve = peers.iter().find(|entry| entry.peer_id == "eve").expect("eve entry");
    assert_eq!(eve.display_name.as_deref(), Some("Eve"));
    assert_eq!(eve.name_source.as_deref(), Some("announce"));
    assert_eq!(eve.trust_level, Some(TrustLevel::Unknown));
    assert!(eve.online);
    assert!(!eve.bootstrap);
}

#[test]
fn peer_directory_consumes_all_contact_and_presence_pages() {
    let app = Client::new(MockBackend::new_paginated());
    let peers = app.peer_directory(None).expect("peer directory");

    assert_eq!(peers.len(), 3);
    assert!(peers.iter().any(|entry| entry.peer_id == "bob"));
    assert!(peers.iter().any(|entry| entry.peer_id == "charlie"));
    assert!(peers.iter().any(|entry| entry.peer_id == "eve"));
}

#[test]
fn client_restarts_by_recreating_inner_client() {
    let backend = MockBackend::new();
    let app = Client::new(backend);
    let first = app.start(Config::desktop_default()).expect("first start");
    app.stop(ShutdownMode::Immediate).expect("stop");
    let second = app.start(Config::desktop_default()).expect("second start");
    assert_ne!(first.runtime_id, second.runtime_id);
}

#[test]
fn client_send_and_status_hide_raw_sdk_types() {
    let backend = MockBackend::new();
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");
    let receipt = app
        .send(
            SendRequest::new("src", "dst", json!({ "body": "hello" }))
                .with_correlation_id("corr-1"),
        )
        .expect("send");
    assert_eq!(receipt.profile, Profile::DesktopDefault);
    assert_eq!(receipt.correlation_id.as_deref(), Some("corr-1"));

    let status = app
        .delivery_status(receipt.message_id.as_str())
        .expect("delivery status")
        .expect("snapshot");
    assert_eq!(status.state, DeliveryState::Sent);
}

#[test]
fn client_status_reports_degraded_after_gap_event() {
    let backend = MockBackend::new();
    backend.queue_batch(RawEventBatch {
        events: vec![runtime_started_event(), stream_gap_event()],
        next_cursor: EventCursor("cursor-2".to_owned()),
        dropped_count: 3,
        snapshot_high_watermark_seq_no: None,
        extensions: BTreeMap::new(),
    });

    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");
    let mut stream = app.subscribe_events(SubscriptionStart::Head).expect("subscribe");
    let batch = stream.next_batch().expect("next batch");
    assert_eq!(batch.events.len(), 2);

    let status = app.status().expect("status");
    assert_eq!(status.state, RunState::Degraded);
}

#[tokio::test]
async fn client_event_stream_yields_typed_events() {
    use tokio_stream::StreamExt;

    let backend = MockBackend::new();
    backend.queue_batch(RawEventBatch {
        events: vec![runtime_started_event()],
        next_cursor: EventCursor("cursor-2".to_owned()),
        dropped_count: 0,
        snapshot_high_watermark_seq_no: None,
        extensions: BTreeMap::new(),
    });

    let app = Client::new(backend);
    app.runtime().start(Config::desktop_default()).expect("start");
    let mut stream = app.events().subscribe(SubscriptionStart::Head).expect("subscribe");

    let event = stream.next().await.expect("stream item").expect("event");
    assert_eq!(event.kind, EventKind::RuntimeStarted);
}

#[tokio::test]
async fn client_event_stream_prefers_native_live_stream() {
    use tokio_stream::StreamExt;

    let backend = MockBackend::new();
    backend.queue_live_event(runtime_started_event());

    let app = Client::new(backend);
    app.runtime().start(Config::desktop_default()).expect("start");
    let mut stream = app.events().subscribe(SubscriptionStart::Head).expect("subscribe");

    let event = stream.next().await.expect("stream item").expect("event");
    assert_eq!(event.kind, EventKind::RuntimeStarted);
}

#[tokio::test]
async fn client_event_stream_dedupes_native_replayed_sequence_numbers() {
    use tokio_stream::StreamExt;

    let backend = MockBackend::new();
    backend.queue_live_event(runtime_started_event());
    backend.queue_live_event(runtime_started_event());
    backend.queue_live_event(stream_gap_event());

    let app = Client::new(backend);
    app.runtime().start(Config::desktop_default()).expect("start");
    let mut stream = app.events().subscribe(SubscriptionStart::Head).expect("subscribe");

    let first = stream.next().await.expect("first item").expect("first event");
    let second = stream.next().await.expect("second item").expect("second event");

    assert_eq!(first.metadata.seq_no, 1);
    assert_eq!(second.metadata.seq_no, 2);
    assert_eq!(first.kind, EventKind::RuntimeStarted);
    assert!(matches!(second.kind, EventKind::StreamGapDetected(_)));
}

#[tokio::test]
async fn client_domain_async_methods_share_runtime_state() {
    let app = Client::new(MockBackend::new());
    let handle = app.runtime().start_async(Config::desktop_default()).await.expect("async start");
    assert_eq!(handle.profile, Profile::DesktopDefault);

    let receipt = app
        .messages()
        .send_async(SendRequest::new("src", "dst", json!({ "body": "hello async" })))
        .await
        .expect("async send");
    assert!(receipt.message_id.starts_with("msg-"));

    let status = app.runtime().status_async().await.expect("async status");
    assert_eq!(status.state, RunState::Running);
}

#[tokio::test]
async fn stop_during_async_start_does_not_install_stale_runtime() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let backend = MockBackend::new();
            let release_start = backend.delay_next_async_negotiate();
            let app = std::sync::Arc::new(Client::new(backend));

            let starting_app = std::sync::Arc::clone(&app);
            let start_task = tokio::task::spawn_local(async move {
                starting_app.runtime().start_async(Config::desktop_default()).await
            });

            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            let err = app
                .runtime()
                .stop(ShutdownMode::Immediate)
                .expect_err("stop should reject while async start is in progress");
            assert_eq!(err.code, crate::app::ErrorCode::RuntimeInvalidState);

            release_start.send(()).expect("release async negotiate");
            let handle = start_task.await.expect("start task").expect("start should complete");
            assert_eq!(handle.profile, Profile::DesktopDefault);

            let status = app.runtime().status().expect("status");
            assert_eq!(status.state, RunState::Running);
        })
        .await;
}

#[test]
fn client_domain_handles_delegate_to_core_app_surface() {
    let app = Client::new(MockBackend::new());
    let handle = app.runtime().start(Config::desktop_default()).expect("start");
    assert_eq!(handle.profile, Profile::DesktopDefault);

    let receipt = app
        .messages()
        .send(SendRequest::new("src", "dst", json!({ "body": "domain" })))
        .expect("send through messages domain");
    assert_eq!(receipt.profile, Profile::DesktopDefault);

    let status =
        app.messages().status(receipt.message_id.clone()).expect("message status").expect("status");
    assert_eq!(status.message_id, receipt.message_id);

    let runtime = app.runtime().status().expect("runtime status");
    assert_eq!(runtime.state, RunState::Running);
}

#[test]
fn client_returns_not_started_before_start() {
    let app = Client::new(MockBackend::new());
    let err = app
        .send(SendRequest::new("src", "dst", json!({ "body": "hello" })))
        .expect_err("send should fail");
    assert_eq!(err.code.as_str(), "SDK_APP_RUNTIME_NOT_STARTED");
    assert!(!err.user_action_required);
}

#[test]
fn failed_stop_preserves_live_session_state() {
    let backend = MockBackend::new();
    backend.queue_shutdown_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "shutdown failed",
    )));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let err = app.stop(ShutdownMode::Immediate).expect_err("stop should fail");
    assert_eq!(err.code.as_str(), "SDK_APP_INTERNAL_UNEXPECTED_FAILURE");

    let receipt = app
        .send(SendRequest::new("src", "dst", json!({ "body": "still-live" })))
        .expect("send after failed stop");
    assert_eq!(receipt.profile, Profile::DesktopDefault);
}

#[test]
fn restart_propagates_stop_failures() {
    let backend = MockBackend::new();
    backend.queue_shutdown_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "shutdown failed",
    )));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let err =
        app.restart(Config::desktop_default()).expect_err("restart should fail when stop fails");
    assert_eq!(err.code.as_str(), "SDK_APP_INTERNAL_UNEXPECTED_FAILURE");
}

#[test]
fn delivery_plan_tracks_profile_defaults() {
    let config = Config::desktop_default();
    let plan = config.delivery_plan();

    assert_eq!(plan.profile, Profile::DesktopDefault);
    assert_eq!(plan.retry.max_attempts, 5);
    assert!(plan.reconnect.enabled);
    assert_eq!(plan.default_event_batch_size, 64);
    assert!(plan.redaction_enabled);
}

#[test]
fn send_with_profile_defaults_retries_queue_pressure() {
    let backend = MockBackend::new();
    backend.queue_send_result(Err(SdkError::new(
        "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
        SdkErrorCategory::Runtime,
        "full",
    )
    .with_retryable(true)));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let report = app
        .send_with_profile_defaults(SendRequest::new("src", "dst", json!({ "body": "hello" })))
        .expect("report");

    assert_eq!(report.attempts.len(), 1);
    assert_eq!(report.attempts[0].disposition, super::super::delivery::AttemptDisposition::Retried);
    assert!(report.attempts[0].queue_pressure);
    assert_eq!(report.receipt.profile, Profile::DesktopDefault);
}

#[test]
fn send_with_options_can_fail_fast_on_queue_pressure() {
    let backend = MockBackend::new();
    backend.queue_send_result(Err(SdkError::new(
        "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
        SdkErrorCategory::Runtime,
        "full",
    )
    .with_retryable(true)));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let err = app
        .send_with_options(
            SendRequest::new("src", "dst", json!({ "body": "hello" })),
            super::super::delivery::DeliveryOptions {
                queue_pressure_strategy: Some(
                    super::super::delivery::QueuePressureStrategy::FailFast,
                ),
                ..Default::default()
            },
        )
        .expect_err("queue pressure should fail fast");

    assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_QUEUE_PRESSURE");
}

#[test]
fn send_with_options_maps_retry_exhaustion() {
    let backend = MockBackend::new();
    backend.queue_send_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "temporary",
    )
    .with_retryable(true)));
    backend.queue_send_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "temporary",
    )
    .with_retryable(true)));
    let app = Client::new(backend);
    app.start(Config::testing_default()).expect("start");

    let err = app
        .send_with_options(
            SendRequest::new("src", "dst", json!({ "body": "hello" })),
            DeliveryOptions { max_attempts: Some(2), ..Default::default() },
        )
        .expect_err("retry exhaustion");

    assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_RETRY_EXHAUSTED");
    assert_eq!(err.cause_code.as_deref(), Some("SDK_INTERNAL_ERROR"));
}
