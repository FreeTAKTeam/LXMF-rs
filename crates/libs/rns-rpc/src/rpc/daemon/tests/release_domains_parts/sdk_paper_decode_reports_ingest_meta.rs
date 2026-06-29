#[test]
fn sdk_paper_decode_reports_ingest_metadata_and_duplicate_scans() {
    let daemon = RpcDaemon::test_instance();
    let destination = hex::decode("00112233445566778899aabbccddeeff").expect("destination");
    let mut paper_bytes = destination;
    paper_bytes.extend_from_slice(b"canonical-paper-payload");
    let uri = format!(
        "lxm://{}",
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, paper_bytes)
    );

    let first = daemon
        .handle_rpc(rpc_request(1, "sdk_paper_decode_v2", json!({ "uri": uri.clone() })))
        .expect("first paper decode");
    assert!(first.error.is_none());
    let first = first.result.expect("first paper result");
    assert_eq!(first["accepted"], json!(true));
    assert_eq!(first["destination"], json!("00112233445566778899aabbccddeeff"));
    assert_eq!(first["destination_hint"], first["destination"]);
    assert!(first["transient_id"].as_str().is_some_and(|value| !value.is_empty()));
    assert_eq!(first["duplicate"], json!(false));
    assert_eq!(first["bytes_len"], json!(uri.len()));

    let duplicate = daemon
        .handle_rpc(rpc_request(2, "sdk_paper_decode_v2", json!({ "uri": uri })))
        .expect("duplicate paper decode");
    assert!(duplicate.error.is_none());
    let duplicate = duplicate.result.expect("duplicate paper result");
    assert_eq!(duplicate["transient_id"], first["transient_id"]);
    assert_eq!(duplicate["destination"], first["destination"]);
    assert_eq!(duplicate["bytes_len"], first["bytes_len"]);
    assert_eq!(duplicate["duplicate"], json!(true));

    let invalid = daemon
        .handle_rpc(rpc_request(3, "sdk_paper_decode_v2", json!({ "uri": "lxm://" })))
        .expect("invalid paper decode");
    assert_eq!(
        invalid.error.expect("destination-less URI must fail").code,
        "SDK_VALIDATION_INVALID_ARGUMENT"
    );
}

#[derive(Debug)]
struct PaperDestinationBridge;

impl OutboundBridge for PaperDestinationBridge {
    fn deliver(
        &self,
        _record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn decode_paper_uri(&self, _uri: &str) -> Result<Option<PaperDecodeOutcome>, std::io::Error> {
        Ok(Some(PaperDecodeOutcome {
            transient_id: "decoded-transient-id".to_owned(),
            destination_hint: "decoded-destination".to_owned(),
            record: None,
            raw_lxmf_bytes: Some(vec![1, 2, 3]),
        }))
    }
}

#[derive(Debug)]
struct PaperRecordBridge;

impl OutboundBridge for PaperRecordBridge {
    fn deliver(
        &self,
        _record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn decode_paper_uri(&self, _uri: &str) -> Result<Option<PaperDecodeOutcome>, std::io::Error> {
        Ok(Some(PaperDecodeOutcome {
            transient_id: "decoded-record-transient-id".to_owned(),
            destination_hint: "decoded-record-destination".to_owned(),
            record: Some(MessageRecord {
                id: "paper-record-message-1".to_owned(),
                source: "paper-source".to_owned(),
                destination: "decoded-record-destination".to_owned(),
                title: "paper title".to_owned(),
                content: "paper body".to_owned(),
                timestamp: now_i64(),
                direction: "in".to_owned(),
                fields: Some(json!({ "paper": true })),
                receipt_status: None,
            }),
            raw_lxmf_bytes: Some(vec![0x10, 0x20, 0x30]),
        }))
    }
}

#[test]
fn sdk_paper_decode_prefers_bridge_destination_over_request_hint() {
    let daemon = RpcDaemon::with_store_and_bridge(
        MessagesStore::in_memory().expect("store"),
        "paper-destination-node".to_owned(),
        Arc::new(PaperDestinationBridge),
    );

    let response = daemon
        .handle_rpc(rpc_request(
            1,
            "sdk_paper_decode_v2",
            json!({
                "uri": "lxm://placeholder/message",
                "destination_hint": "caller-destination",
            }),
        ))
        .expect("paper decode");
    assert!(response.error.is_none());
    let result = response.result.expect("paper result");
    assert_eq!(result["destination"], json!("decoded-destination"));
    assert_eq!(result["destination_hint"], json!("decoded-destination"));
    assert_eq!(result["bytes_len"], json!(3));
}

#[test]
fn sdk_paper_decode_bridge_record_persists_and_emits_raw_inbound_event() {
    let daemon = RpcDaemon::with_store_and_bridge(
        MessagesStore::in_memory().expect("store"),
        "paper-record-node".to_owned(),
        Arc::new(PaperRecordBridge),
    );

    let pre_poll = daemon
        .handle_rpc(rpc_request(
            1,
            "sdk_poll_events_v2",
            json!({ "cursor": null, "max": 50 }),
        ))
        .expect("pre poll");
    assert!(pre_poll.error.is_none());
    let pre_cursor = pre_poll.result.expect("pre poll result")["next_cursor"]
        .as_str()
        .expect("pre cursor")
        .to_owned();

    let response = daemon
        .handle_rpc(rpc_request(
            2,
            "sdk_paper_decode_v2",
            json!({
                "uri": "lxm://placeholder/record",
                "destination_hint": "caller-destination",
            }),
        ))
        .expect("paper decode");
    assert!(response.error.is_none());
    let result = response.result.expect("paper result");
    assert_eq!(result["accepted"], json!(true));
    assert_eq!(result["duplicate"], json!(false));
    assert_eq!(result["transient_id"], json!("decoded-record-transient-id"));
    assert_eq!(result["destination"], json!("decoded-record-destination"));
    assert_eq!(result["bytes_len"], json!(3));
    assert!(
        daemon.message_exists("paper-record-message-1").expect("message exists"),
        "bridge-backed paper decode should persist the decoded inbound record"
    );

    let poll = daemon
        .handle_rpc(rpc_request(
            3,
            "sdk_poll_events_v2",
            json!({ "cursor": pre_cursor, "max": 50 }),
        ))
        .expect("poll events");
    assert!(poll.error.is_none());
    let poll_result = poll.result.expect("poll result");
    let events = poll_result["events"].as_array().expect("events");
    let inbound = events
        .iter()
        .find(|event| event["event_type"] == json!("inbound"))
        .expect("inbound paper event");
    assert_eq!(inbound["payload"]["message"]["id"], json!("paper-record-message-1"));
    assert_eq!(inbound["payload"]["lxmf_bytes_hex"], json!("102030"));

    let duplicate = daemon
        .handle_rpc(rpc_request(
            4,
            "sdk_paper_decode_v2",
            json!({ "uri": "lxm://placeholder/record" }),
        ))
        .expect("duplicate paper decode");
    assert!(duplicate.error.is_none());
    let duplicate = duplicate.result.expect("duplicate paper result");
    assert_eq!(duplicate["duplicate"], json!(true));
    assert_eq!(duplicate["transient_id"], json!("decoded-record-transient-id"));
}

#[test]
fn sdk_paper_decode_rejects_empty_uri_before_calling_bridge() {
    let daemon = RpcDaemon::with_store_and_bridge(
        MessagesStore::in_memory().expect("store"),
        "paper-validation-node".to_owned(),
        Arc::new(PaperDestinationBridge),
    );

    let response = daemon
        .handle_rpc(rpc_request(1, "sdk_paper_decode_v2", json!({ "uri": "lxm://" })))
        .expect("paper decode validation");
    assert_eq!(
        response.error.expect("empty paper URI must fail validation").code,
        "SDK_VALIDATION_INVALID_ARGUMENT"
    );
}

#[test]
fn sdk_operation_registry_roundtrips_workflow_family() {
    let daemon = RpcDaemon::test_instance();

    let peer_ready = daemon
        .handle_rpc(rpc_request(
            1360,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_workflow_peer_ready_v2",
                "kind": "command",
                "payload": {
                    "identity": "node-b",
                    "display_name": "Node Bravo",
                    "trust_level": "trusted",
                    "bootstrap": true,
                    "announce": true,
                },
            }),
        ))
        .expect("workflow peer ready");
    assert!(peer_ready.error.is_none());
    let peer_payload = &peer_ready.result.expect("peer ready result")["response"]["payload"];
    assert_eq!(peer_payload["contact"]["identity"], json!("node-b"));
    assert_eq!(peer_payload["announced"], json!(true));

    let topic_sync = daemon
        .handle_rpc(rpc_request(
            1361,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_workflow_topic_sync_v2",
                "kind": "command",
                "payload": {
                    "topic_path": "ops/workflow",
                    "telemetry_limit": 0,
                },
            }),
        ))
        .expect("workflow topic sync");
    assert!(topic_sync.error.is_none());
    let topic_payload = &topic_sync.result.expect("topic sync result")["response"]["payload"];
    assert_eq!(topic_payload["topic"]["topic_path"], json!("ops/workflow"));
    assert_eq!(topic_payload["subscribed"], json!(true));

    let attachment_report = daemon
        .handle_rpc(rpc_request(
            1362,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_workflow_attachment_report_publish_v2",
                "kind": "command",
                "payload": {
                    "topic_path": "ops/workflow",
                    "summary_payload": { "summary": "workflow report" },
                    "attachment": {
                        "name": "report.txt",
                        "content_type": "text/plain",
                        "bytes_base64": "cmVwb3J0",
                    },
                },
            }),
        ))
        .expect("workflow attachment report");
    assert!(attachment_report.error.is_none());
    let attachment_payload =
        &attachment_report.result.expect("attachment report result")["response"]["payload"];
    assert_eq!(attachment_payload["attachment"]["name"], json!("report.txt"));
    assert_eq!(attachment_payload["published"]["accepted"], json!(true));

    let mission = daemon
        .handle_rpc(rpc_request(
            1363,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_workflow_mission_update_send_v2",
                "kind": "command",
                "payload": {
                    "peer_identity": "node-b",
                    "content": "mission update",
                    "topic_path": "ops/workflow",
                    "attachments": [{
                        "name": "sitrep.txt",
                        "content_type": "text/plain",
                        "bytes_base64": "c2l0cmVw",
                    }],
                },
            }),
        ))
        .expect("workflow mission update");
    assert!(mission.error.is_none());
    let mission_payload = &mission.result.expect("mission update result")["response"]["payload"];
    assert_eq!(mission_payload["topic"]["topic_path"], json!("ops/workflow"));
    assert_eq!(mission_payload["attachments"].as_array().expect("attachments").len(), 1);
    assert!(mission_payload["message_id"].as_str().is_some());
}

#[test]
fn sdk_command_events_summarize_large_payloads() {
    let daemon = RpcDaemon::test_instance();

    let pre_poll = daemon
        .handle_rpc(rpc_request(
            490,
            "sdk_poll_events_v2",
            json!({ "cursor": null, "max": 50 }),
        ))
        .expect("pre poll");
    assert!(pre_poll.error.is_none());
    let pre_cursor = pre_poll.result.expect("pre poll result")["next_cursor"]
        .as_str()
        .expect("pre cursor")
        .to_string();

    let large_body = "x".repeat(64 * 1024);
    let command = daemon
        .handle_rpc(rpc_request(
            491,
            "sdk_command_invoke_v2",
            json!({
                "command": "large",
                "target": "node-b",
                "payload": { "body": large_body },
            }),
        ))
        .expect("command invoke");
    assert!(command.error.is_none());

    let poll = daemon
        .handle_rpc(rpc_request(
            492,
            "sdk_poll_events_v2",
            json!({ "cursor": pre_cursor, "max": 50 }),
        ))
        .expect("poll");
    assert!(poll.error.is_none());
    let events = poll.result.expect("poll result")["events"].as_array().expect("events").clone();
    let dispatched = events
        .iter()
        .find(|event| event["event_type"] == json!("command.dispatched"))
        .expect("command.dispatched event");
    assert_eq!(dispatched["payload"]["request_payload"]["kind"], json!("object"));
    assert_eq!(dispatched["payload"]["request_payload"]["truncated"], json!(true));
}

#[test]
fn sdk_operation_registry_includes_product_catalog_entries() {
    let daemon =
        RpcDaemon::test_instance().with_sdk_custom_operations(vec![SdkCustomOperationSpec::new(
            "r3akt.message.send",
            "r3akt",
            "command",
            "extension",
            "Send a R3AKT product message through the shared operation runtime.",
        )
        .with_alias("R3AKT;EMergencyMessages.send")]);

    let registry = daemon
        .handle_rpc(rpc_request(1318, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let entries = registry.result.expect("registry result")["registry"]["entries"]
        .as_array()
        .expect("registry entries")
        .clone();
    let custom = entries
        .iter()
        .find(|entry| entry["id"] == json!("r3akt.message.send"))
        .expect("custom operation entry");
    assert_eq!(custom["group"], json!("r3akt"));
    assert_eq!(custom["transport_variant"], json!("extension"));
    assert_eq!(custom["aliases"][0], json!("R3AKT;EMergencyMessages.send"));

    let response = daemon
        .handle_rpc(rpc_request(
            1319,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "R3AKT;EMergencyMessages.send",
                "kind": "command",
                "target": "node-b",
                "payload": { "text": "hello" },
                "extensions": { "product": "r3akt" }
            }),
        ))
        .expect("custom operation envelope");
    assert!(response.error.is_none());
    let response = response.result.expect("custom envelope result");
    assert_eq!(response["response"]["operation_id"], json!("r3akt.message.send"));
    assert_eq!(response["response"]["payload"]["command"], json!("r3akt.message.send"));
    assert_eq!(response["response"]["extensions"]["product"], json!("r3akt"));
}

#[test]
fn sdk_negotiate_v2_installs_product_catalog_entries() {
    let daemon = RpcDaemon::test_instance();
    let negotiated = daemon
        .handle_rpc(rpc_request(
            1316,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "extensions": {
                        "custom_operations": [{
                            "id": "r3akt.message.send",
                            "group": "r3akt",
                            "kind": "command",
                            "transport_variant": "extension",
                            "description": "Send a R3AKT product message through the shared operation runtime.",
                            "aliases": ["R3AKT;EMergencyMessages.send"]
                        }]
                    }
                }
            }),
        ))
        .expect("negotiate with custom operation catalog");
    assert!(negotiated.error.is_none());

    let registry = daemon
        .handle_rpc(rpc_request(1317, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let entries = registry.result.expect("registry result")["registry"]["entries"]
        .as_array()
        .expect("registry entries")
        .clone();
    assert!(
        entries.iter().any(|entry| {
            entry["id"] == json!("r3akt.message.send")
                && entry["aliases"][0] == json!("R3AKT;EMergencyMessages.send")
        }),
        "startup product catalog should be visible through the daemon registry"
    );
}

#[test]
fn sdk_operation_registry_roundtrips_topic_family() {
    let daemon = RpcDaemon::test_instance();

    let registry = daemon
        .handle_rpc(rpc_request(1320, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let registry_result = registry.result.expect("registry result");
    let entries = registry_result["registry"]["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.topic.create")));
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.topic.publish")));

    let topic_create = daemon
        .handle_rpc(rpc_request(
            1321,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_topic_create_v2",
                "kind": "command",
                "payload": {
                    "topic_path": "ops/envelope",
                    "metadata": { "kind": "ops" },
                },
            }),
        ))
        .expect("topic create envelope");
    assert!(topic_create.error.is_none());
    let topic_payload = &topic_create.result.expect("topic create result")["response"]["payload"];
    let topic_id = topic_payload["topic_id"].as_str().expect("topic id").to_string();
    assert_eq!(topic_payload["topic_path"], json!("ops/envelope"));

    let topic_list = daemon
        .handle_rpc(rpc_request(
            1322,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.topic.list",
                "kind": "query",
                "payload": { "limit": 10 },
            }),
        ))
        .expect("topic list envelope");
    assert!(topic_list.error.is_none());
    assert!(!topic_list.result.expect("topic list result")["response"]["payload"]["topics"]
        .as_array()
        .expect("topics")
        .is_empty());

    let topic_publish = daemon
        .handle_rpc(rpc_request(
            1323,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.topic.publish",
                "kind": "command",
                "payload": {
                    "topic_id": topic_id,
                    "payload": { "message": "hello topic" },
                    "correlation_id": "topic-env-1",
                },
            }),
        ))
        .expect("topic publish envelope");
    assert!(topic_publish.error.is_none());
    assert_eq!(
        topic_publish.result.expect("topic publish result")["response"]["payload"]["accepted"],
        json!(true)
    );
}
