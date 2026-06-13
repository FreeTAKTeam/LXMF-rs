#[test]
fn execute_envelope_routes_delivery_send_locally() {
    let backend = MockBackend::new();
    backend.queue_send_result(Ok(crate::MessageId("msg-1".to_owned())));
    let app = Client::new(backend);
    app.start(Config::testing_default()).expect("start");

    let response = app
        .command(
            "app.delivery.send",
            serde_json::json!({
                "source": "src",
                "destination": "dst",
                "payload": { "content": "hello" },
                "correlation_id": "corr-1"
            }),
        )
        .expect("delivery send");
    assert_eq!(response.operation_id.as_str(), "app.delivery.send");
    assert_eq!(response.payload.get("message_id").and_then(|value| value.as_str()), Some("msg-1"));
}

#[test]
fn execute_envelope_routes_custom_commands_via_remote_command_backend() {
    let backend = MockBackend::new();
    backend.queue_remote_command_result(Ok(crate::domain::RemoteCommandResponse {
        accepted: true,
        payload: serde_json::json!({
            "command_id": "cmdreq-1",
            "correlation_id": "cmd-1",
            "command": "vendor.example.custom",
            "target": null,
            "command_state": "dispatched",
        }),
        extensions: BTreeMap::from([("transport".to_owned(), serde_json::json!("remote"))]),
    }));
    let app = Client::new(backend);
    app.start(Config::desktop_default().with_custom_operation(OperationEntry::new(
        "vendor.example.custom",
        "custom",
        OperationKind::Command,
        TransportVariant::Extension,
        "Custom vendor command.",
    )))
    .expect("start");
    let response = app
        .command("vendor.example.custom", serde_json::json!({ "value": 1 }))
        .expect("custom command");
    assert_eq!(response.operation_id.as_str(), "vendor.example.custom");
    assert_eq!(
        response.payload.get("command_state").and_then(|value| value.as_str()),
        Some("dispatched")
    );
    assert_eq!(
        response.extensions.get("transport").and_then(|value| value.as_str()),
        Some("remote")
    );
}

#[test]
fn execute_envelope_rejects_kind_mismatches() {
    let app = Client::new(MockBackend::new());
    let err = app
        .execute_envelope(Envelope::command("app.identity.list", serde_json::json!({})))
        .expect_err("kind mismatch should fail");
    assert_eq!(err.code.as_str(), "SDK_APP_VALIDATION_INVALID_ARGUMENT");
}

#[test]
fn execute_envelope_routes_unhandled_queries_to_backend_envelope_path() {
    let backend = MockBackend::new();
    backend.queue_envelope_result(Ok(crate::app::EnvelopeResponse {
        operation_id: crate::app::OperationId::from("app.message.history.list"),
        kind: crate::app::EnvelopeKind::Result,
        accepted: true,
        correlation_id: Some("corr-1".to_owned()),
        payload: serde_json::json!({ "messages": [] }),
        extensions: BTreeMap::from([("via".to_owned(), serde_json::json!("envelope"))]),
    }));
    let app = Client::new(backend);
    let response = app
        .query("app.message.history.list", serde_json::json!({ "limit": 10 }))
        .expect("history query");
    assert_eq!(response.operation_id.as_str(), "app.message.history.list");
    assert_eq!(response.extensions.get("via").and_then(|value| value.as_str()), Some("envelope"));
}

#[test]
fn execute_envelope_routes_voice_operations_locally() {
    let backend = MockBackend::new();
    backend.queue_voice_open_result(Ok(crate::domain::VoiceSessionId("voice-9".to_owned())));
    backend.queue_voice_update_result(Ok(crate::domain::VoiceSessionState::Active));
    backend.queue_voice_close_result(Ok(Ack { accepted: true, revision: None }));
    let app = Client::new(backend);

    let opened = app
        .command(
            "app.voice.session.open",
            serde_json::json!({ "peer_id": "node-b", "codec_hint": "opus" }),
        )
        .expect("voice open");
    assert_eq!(opened.operation_id.as_str(), "app.voice.session.open");
    assert_eq!(
        serde_json::from_value::<crate::domain::VoiceSessionId>(opened.payload).expect("voice id"),
        crate::domain::VoiceSessionId("voice-9".to_owned())
    );

    let updated = app
        .command(
            "app.voice.session.update",
            serde_json::json!({ "session_id": "voice-9", "state": "active" }),
        )
        .expect("voice update");
    assert_eq!(updated.operation_id.as_str(), "app.voice.session.update");
    assert_eq!(
        serde_json::from_value::<crate::domain::VoiceSessionState>(updated.payload)
            .expect("voice state"),
        crate::domain::VoiceSessionState::Active
    );

    let closed =
        app.command("app.voice.session.close", serde_json::json!("voice-9")).expect("voice close");
    assert_eq!(closed.operation_id.as_str(), "app.voice.session.close");
    assert_eq!(closed.payload.get("accepted").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(closed.payload.get("session_id").and_then(|value| value.as_str()), Some("voice-9"));
}

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
            ContactUpdate::new("charlie")
                .with_display_name("Charlie")
                .with_trust_level(TrustLevel::Untrusted)
                .with_bootstrap(true),
        )
        .expect("contact update");
    assert_eq!(updated.identity, "charlie");
    assert_eq!(updated.display_name.as_deref(), Some("Charlie"));
    assert_eq!(updated.trust_level, TrustLevel::Untrusted);
    assert!(updated.bootstrap);

    let bootstrapped = app.bootstrap_identity(BootstrapRequest::new("delta")).expect("bootstrap");
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
fn peer_directory_limit_preserves_presence_for_returned_contacts() {
    let app = Client::new(MockBackend::new_paginated());
    let peers = app.peer_directory(Some(1)).expect("peer directory");

    assert_eq!(peers.len(), 1);
    let peer = &peers[0];
    assert_eq!(peer.peer_id, "bob");
    assert!(peer.online);
    assert_eq!(peer.last_seen_ts_ms, Some(200));
    assert_eq!(peer.first_seen_ts_ms, Some(120));
    assert_eq!(peer.seen_count, 3);
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
    assert_eq!(report.attempts[0].disposition, AttemptDisposition::Retried);
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
            DeliveryOptions {
                queue_pressure_strategy: Some(QueuePressureStrategy::FailFast),
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
