#[test]
fn execute_envelope_routes_marker_operations_locally() {
    let app = Client::new(MockBackend::new());

    let created = app
        .command(
            "sdk_marker_create_v2",
            serde_json::json!({
                "label": "Alpha",
                "position": { "lat": 35.0, "lon": -115.0, "alt_m": 1200.0 },
                "topic_id": "topic-1",
            }),
        )
        .expect("marker create");
    assert_eq!(created.operation_id.as_str(), "app.marker.create");
    assert_eq!(created.payload["marker_id"], json!("marker-1"));

    let listed = app
        .query(
            "app.marker.list",
            serde_json::json!({
                "topic_id": "topic-1",
                "limit": 10,
            }),
        )
        .expect("marker list");
    assert_eq!(listed.payload["markers"].as_array().expect("marker rows").len(), 1);

    let updated = app
        .command(
            "sdk_marker_update_position_v2",
            serde_json::json!({
                "marker_id": "marker-1",
                "expected_revision": 2,
                "position": { "lat": 36.0, "lon": -116.0, "alt_m": null },
            }),
        )
        .expect("marker update");
    assert_eq!(updated.operation_id.as_str(), "app.marker.update_position");
    assert_eq!(updated.payload["revision"], json!(3));

    let deleted = app
        .command(
            "app.marker.delete",
            serde_json::json!({
                "marker_id": "marker-1",
                "expected_revision": 3,
            }),
        )
        .expect("marker delete");
    assert_eq!(deleted.operation_id.as_str(), "app.marker.delete");
    assert_eq!(deleted.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_runtime_start_and_stop_locally() {
    let app = Client::new(MockBackend::new());
    let start = app
        .command(
            "app.runtime.start",
            serde_json::to_value(Config::testing_default()).expect("config value"),
        )
        .expect("runtime start");
    assert_eq!(start.operation_id.as_str(), "app.runtime.start");
    assert_eq!(
        start.payload.get("profile").and_then(|value| value.as_str()),
        Some("testing_default")
    );

    let stop = app
        .command("app.runtime.stop", serde_json::json!({ "mode": "graceful" }))
        .expect("runtime stop");
    assert_eq!(stop.operation_id.as_str(), "app.runtime.stop");
    assert_eq!(stop.payload.get("accepted").and_then(|value| value.as_bool()), Some(true));
}

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
