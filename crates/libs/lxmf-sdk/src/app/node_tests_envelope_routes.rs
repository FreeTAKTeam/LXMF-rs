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
fn config_start_request_carries_custom_operations_for_rpc_daemon() {
    let request = Config::testing_default()
        .with_custom_operation(
            OperationEntry::new(
                "r3akt.message.send",
                "r3akt",
                OperationKind::Command,
                TransportVariant::Extension,
                "Send a R3AKT product message through the shared operation runtime.",
            )
            .with_alias("R3AKT;EMergencyMessages.send"),
        )
        .start_request();

    let operations = request.config.extensions["custom_operations"]
        .as_array()
        .expect("custom operations extension");
    assert_eq!(operations[0]["id"], json!("r3akt.message.send"));
    assert_eq!(operations[0]["aliases"][0], json!("R3AKT;EMergencyMessages.send"));
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
