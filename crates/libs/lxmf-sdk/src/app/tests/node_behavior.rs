use super::node_support::*;

#[test]
fn config_presets_map_to_expected_profiles() {
    assert_eq!(Config::mobile_default().profile, Profile::MobileDefault);
    assert_eq!(Config::mobile_default().sdk_config.profile, CoreProfile::DesktopLocalRuntime);
    assert_eq!(Config::desktop_default().sdk_config.profile, CoreProfile::DesktopFull);
    assert_eq!(Config::embedded_default().sdk_config.profile, CoreProfile::EmbeddedAlloc);
}

#[test]
fn config_operation_registry_merges_custom_entries() {
    let config = Config::testing_default().with_custom_operation(OperationEntry::new(
        "vendor.example.custom",
        "custom",
        OperationKind::Command,
        TransportVariant::Extension,
        "Custom vendor command.",
    ));
    let registry = config.operation_registry().expect("registry");
    assert!(registry.supports("vendor.example.custom"));
    assert!(registry.supports("sdk_poll_events_v2"));
}

#[test]
fn client_exposes_built_in_registry_before_start() {
    let app = Client::new(MockBackend::new());
    let registry = app.operation_registry().expect("registry");
    assert_eq!(
        registry.canonicalize("sdk_identity_contact_list_v2").expect("canonical id").as_str(),
        "app.contact.list"
    );
}

#[test]
fn execute_envelope_routes_runtime_status_locally() {
    let app = Client::new(MockBackend::new());
    let response = app.query("app.runtime.status", serde_json::json!({})).expect("runtime status");
    assert_eq!(response.kind, EnvelopeKind::Result);
    assert_eq!(response.operation_id.as_str(), "app.runtime.status");
    assert_eq!(response.payload.get("state").and_then(|value| value.as_str()), Some("new"));
}

#[test]
fn execute_envelope_routes_identity_queries_to_backend() {
    let app = Client::new(MockBackend::new());
    let response = app.query("app.identity.list", serde_json::json!({})).expect("identity list");
    let identities = response.payload.as_array().expect("identity array");
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].get("display_name").and_then(|value| value.as_str()), Some("Alice"));
}

#[test]
fn execute_envelope_accepts_registered_aliases() {
    let app = Client::new(MockBackend::new());
    let response =
        app.query("sdk_identity_list_v2", serde_json::json!({})).expect("identity list via alias");
    assert_eq!(response.operation_id.as_str(), "app.identity.list");
    let identities = response.payload.as_array().expect("identity array");
    assert_eq!(identities[0]["identity"], json!("alice"));
}

#[test]
fn execute_envelope_routes_discovery_operations_locally() {
    let app = Client::new(MockBackend::new());

    let announce =
        app.command("sdk_identity_announce_now_v2", serde_json::json!({})).expect("announce");
    assert_eq!(announce.operation_id.as_str(), "app.identity.announce");
    assert_eq!(announce.payload["accepted"], json!(true));

    let presence = app
        .query("sdk_identity_presence_list_v2", serde_json::json!({ "limit": 10 }))
        .expect("presence");
    assert_eq!(presence.operation_id.as_str(), "app.identity.presence.list");
    assert_eq!(presence.payload["peers"].as_array().expect("peer rows").len(), 2);

    let contact = app
        .command(
            "sdk_identity_contact_update_v2",
            serde_json::json!({
                "identity": "charlie",
                "display_name": "Charlie",
                "trust_level": "trusted",
                "bootstrap": true
            }),
        )
        .expect("contact update");
    assert_eq!(contact.operation_id.as_str(), "app.contact.update");
    assert_eq!(contact.payload["identity"], json!("charlie"));

    let bootstrap = app
        .command(
            "sdk_identity_bootstrap_v2",
            serde_json::json!({ "identity": "delta", "auto_sync": true }),
        )
        .expect("bootstrap");
    assert_eq!(bootstrap.operation_id.as_str(), "app.identity.bootstrap");
    assert_eq!(bootstrap.payload["identity"], json!("delta"));
}

#[test]
fn execute_envelope_routes_topic_operations_locally() {
    let app = Client::new(MockBackend::new());

    let topic = app
        .command(
            "sdk_topic_create_v2",
            serde_json::json!({
                "topic_path": "ops/alerts",
                "metadata": { "kind": "ops" }
            }),
        )
        .expect("topic create");
    assert_eq!(topic.operation_id.as_str(), "app.topic.create");
    assert_eq!(topic.payload["topic_id"], json!("topic-1"));

    let fetched = app.query("sdk_topic_get_v2", serde_json::json!("topic-1")).expect("topic get");
    assert_eq!(fetched.operation_id.as_str(), "app.topic.get");
    assert_eq!(fetched.payload["topic_path"], json!("ops/alerts"));

    let listed =
        app.query("app.topic.list", serde_json::json!({ "limit": 10 })).expect("topic list");
    assert_eq!(listed.payload["topics"].as_array().expect("topic list").len(), 1);
    assert_eq!(listed.payload["next_cursor"], json!("topic:1"));

    let subscribed = app
        .command("sdk_topic_subscribe_v2", serde_json::json!({ "topic_id": "topic-1" }))
        .expect("topic subscribe");
    assert_eq!(subscribed.operation_id.as_str(), "app.topic.subscribe");
    assert_eq!(subscribed.payload["accepted"], json!(true));

    let published = app
        .command(
            "app.topic.publish",
            serde_json::json!({
                "topic_id": "topic-1",
                "payload": { "message": "hello topic" },
                "correlation_id": "topic-corr-1"
            }),
        )
        .expect("topic publish");
    assert_eq!(published.operation_id.as_str(), "app.topic.publish");
    assert_eq!(published.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_workflow_operations_locally() {
    let app = Client::new(MockBackend::new());
    app.start(Config::testing_default()).expect("start");

    let peer_ready = app
        .command(
            "sdk_workflow_peer_ready_v2",
            serde_json::json!({
                "identity": "delta",
                "announce": true,
                "bootstrap": true,
            }),
        )
        .expect("workflow peer ready");
    assert_eq!(peer_ready.operation_id.as_str(), "app.workflow.peer_ready");
    assert_eq!(peer_ready.payload["contact"]["identity"], json!("delta"));

    let topic_sync = app
        .command(
            "sdk_workflow_topic_sync_v2",
            serde_json::json!({
                "topic_path": "ops/alerts",
                "telemetry_limit": 5,
            }),
        )
        .expect("workflow topic sync");
    assert_eq!(topic_sync.operation_id.as_str(), "app.workflow.topic_sync");
    assert_eq!(topic_sync.payload["topic"]["topic_id"], json!("topic-1"));
    assert_eq!(topic_sync.payload["subscribed"], json!(true));

    let mission = app
        .command(
            "sdk_workflow_mission_update_send_v2",
            serde_json::json!({
                "peer_identity": "delta",
                "content": "mission update",
                "topic_path": "ops/alerts",
                "attachments": [{
                    "name": "sitrep.txt",
                    "content_type": "text/plain",
                    "bytes_base64": "c2l0cmVw",
                }],
            }),
        )
        .expect("workflow mission update");
    assert_eq!(mission.operation_id.as_str(), "app.workflow.mission_update_send");
    assert_eq!(mission.payload["message_id"], json!("msg-1"));
    assert_eq!(mission.payload["attachments"].as_array().expect("attachments").len(), 1);
}

#[test]
fn execute_envelope_rejects_reserved_mission_metadata() {
    let app = Client::new(MockBackend::new());

    let err = app.command(
        "sdk_workflow_mission_update_send_v2",
        serde_json::json!({
            "peer_identity": "delta",
            "content": "mission update",
            "metadata": {
                "topic_id": "override",
            },
        }),
    );
    assert!(err.is_err());
}

#[test]
fn execute_envelope_routes_telemetry_operations_locally() {
    let app = Client::new(MockBackend::new());

    let telemetry = app
        .query(
            "sdk_telemetry_query_v2",
            serde_json::json!({
                "topic_id": "topic-1",
                "peer_id": "node-b",
                "from_ts_ms": 100,
                "limit": 10,
            }),
        )
        .expect("telemetry query");
    assert_eq!(telemetry.operation_id.as_str(), "app.telemetry.query");
    assert_eq!(telemetry.payload.as_array().expect("telemetry rows").len(), 1);
    assert_eq!(telemetry.payload[0]["tags"]["topic_id"], json!("topic-1"));

    let subscribed = app
        .command(
            "app.telemetry.subscribe",
            serde_json::json!({
                "topic_id": "topic-1",
                "from_ts_ms": 100,
                "limit": 20,
            }),
        )
        .expect("telemetry subscribe");
    assert_eq!(subscribed.operation_id.as_str(), "app.telemetry.subscribe");
    assert_eq!(subscribed.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_attachment_operations_locally() {
    let app = Client::new(MockBackend::new());

    let stored = app
        .command(
            "sdk_attachment_store_v2",
            serde_json::json!({
                "name": "sample.txt",
                "content_type": "text/plain",
                "bytes_base64": "aGVsbG8gd29ybGQ=",
                "topic_ids": ["topic-1"],
            }),
        )
        .expect("attachment store");
    assert_eq!(stored.operation_id.as_str(), "app.attachment.store");
    assert_eq!(stored.payload["attachment_id"], json!("attachment-1"));

    let fetched = app
        .query("sdk_attachment_get_v2", serde_json::json!("attachment-1"))
        .expect("attachment get");
    assert_eq!(fetched.operation_id.as_str(), "app.attachment.get");
    assert_eq!(fetched.payload["name"], json!("sample.txt"));

    let listed = app
        .query(
            "app.attachment.list",
            serde_json::json!({
                "topic_id": "topic-1",
                "limit": 10,
            }),
        )
        .expect("attachment list");
    assert_eq!(listed.payload["attachments"].as_array().expect("attachment rows").len(), 1);

    let associated = app
        .command(
            "sdk_attachment_associate_topic_v2",
            serde_json::json!({
                "attachment_id": "attachment-1",
                "topic_id": "topic-2",
            }),
        )
        .expect("attachment associate");
    assert_eq!(associated.operation_id.as_str(), "app.attachment.associate_topic");
    assert_eq!(associated.payload["accepted"], json!(true));

    let deleted = app
        .command("app.attachment.delete", serde_json::json!("attachment-1"))
        .expect("attachment delete");
    assert_eq!(deleted.operation_id.as_str(), "app.attachment.delete");
    assert_eq!(deleted.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_attachment_streaming_operations_locally() {
    let app = Client::new(MockBackend::new());

    let upload = app
            .command(
                "sdk_attachment_upload_start_v2",
                serde_json::json!({
                    "name": "chunked.bin",
                    "content_type": "application/octet-stream",
                    "total_size": 11,
                    "checksum_sha256": "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c",
                    "topic_ids": ["topic-1"],
                }),
            )
            .expect("attachment upload start");
    assert_eq!(upload.operation_id.as_str(), "app.attachment.upload_start");
    assert_eq!(upload.payload["upload_id"], json!("upload-1"));

    let chunk = app
        .command(
            "app.attachment.upload_chunk",
            serde_json::json!({
                "upload_id": "upload-1",
                "offset": 0,
                "bytes_base64": "aGVsbG8=",
            }),
        )
        .expect("attachment upload chunk");
    assert_eq!(chunk.operation_id.as_str(), "app.attachment.upload_chunk");
    assert_eq!(chunk.payload["accepted"], json!(true));

    let committed = app
        .command(
            "sdk_attachment_upload_commit_v2",
            serde_json::json!({
                "upload_id": "upload-1",
            }),
        )
        .expect("attachment upload commit");
    assert_eq!(committed.operation_id.as_str(), "app.attachment.upload_commit");
    assert_eq!(committed.payload["attachment_id"], json!("attachment-1"));

    let downloaded = app
        .query(
            "sdk_attachment_download_chunk_v2",
            serde_json::json!({
                "attachment_id": "attachment-1",
                "offset": 0,
                "max_bytes": 5,
            }),
        )
        .expect("attachment download chunk");
    assert_eq!(downloaded.operation_id.as_str(), "app.attachment.download_chunk");
    assert_eq!(downloaded.payload["next_offset"], json!(5));
    assert_eq!(downloaded.payload["done"], json!(false));
}

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
