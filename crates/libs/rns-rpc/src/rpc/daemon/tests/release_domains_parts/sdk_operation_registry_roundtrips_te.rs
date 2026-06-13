#[test]
fn sdk_operation_registry_roundtrips_telemetry_family() {
    let daemon = RpcDaemon::test_instance();

    let topic = daemon
        .handle_rpc(rpc_request(
            1330,
            "sdk_topic_create_v2",
            json!({ "topic_path": "ops/telemetry" }),
        ))
        .expect("topic create");
    let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
        .as_str()
        .expect("topic id")
        .to_string();
    let _publish = daemon
        .handle_rpc(rpc_request(
            1331,
            "sdk_topic_publish_v2",
            json!({
                "topic_id": topic_id.clone(),
                "payload": { "message": "hello telemetry" },
                "correlation_id": "telemetry-corr-1",
            }),
        ))
        .expect("topic publish");

    let registry = daemon
        .handle_rpc(rpc_request(1332, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let registry_result = registry.result.expect("registry result");
    let entries = registry_result["registry"]["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.telemetry.query")));
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.telemetry.subscribe")));

    let telemetry_query = daemon
        .handle_rpc(rpc_request(
            1333,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_telemetry_query_v2",
                "kind": "query",
                "payload": {
                    "topic_id": topic_id,
                    "from_ts_ms": 0,
                    "limit": 10,
                },
            }),
        ))
        .expect("telemetry query envelope");
    assert!(telemetry_query.error.is_none());
    let telemetry_payload =
        &telemetry_query.result.expect("telemetry query result")["response"]["payload"];
    assert!(!telemetry_payload.as_array().expect("telemetry points").is_empty());

    let telemetry_subscribe = daemon
        .handle_rpc(rpc_request(
            1334,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.telemetry.subscribe",
                "kind": "command",
                "payload": {
                    "peer_id": "node-b",
                    "from_ts_ms": 0,
                    "limit": 10,
                },
            }),
        ))
        .expect("telemetry subscribe envelope");
    assert!(telemetry_subscribe.error.is_none());
    assert_eq!(
        telemetry_subscribe.result.expect("telemetry subscribe result")["response"]["payload"]
            ["accepted"],
        json!(true)
    );
}

#[test]
fn sdk_operation_registry_roundtrips_attachment_family() {
    let daemon = RpcDaemon::test_instance();

    let topic = daemon
        .handle_rpc(rpc_request(
            1339,
            "sdk_topic_create_v2",
            json!({ "topic_path": "ops/attachments" }),
        ))
        .expect("topic create");
    let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
        .as_str()
        .expect("topic id")
        .to_string();

    let registry = daemon
        .handle_rpc(rpc_request(1340, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let registry_result = registry.result.expect("registry result");
    let entries = registry_result["registry"]["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.attachment.store")));
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.attachment.delete")));

    let attachment_store = daemon
        .handle_rpc(rpc_request(
            1341,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_attachment_store_v2",
                "kind": "command",
                "payload": {
                    "name": "sample.txt",
                    "content_type": "text/plain",
                    "bytes_base64": "aGVsbG8gd29ybGQ=",
                    "topic_ids": [topic_id.clone()],
                },
            }),
        ))
        .expect("attachment store envelope");
    assert!(attachment_store.error.is_none());
    let stored_payload =
        &attachment_store.result.expect("attachment store result")["response"]["payload"];
    let attachment_id =
        stored_payload["attachment_id"].as_str().expect("attachment id").to_string();

    let attachment_get = daemon
        .handle_rpc(rpc_request(
            1342,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.attachment.get",
                "kind": "query",
                "payload": attachment_id,
            }),
        ))
        .expect("attachment get envelope");
    assert!(attachment_get.error.is_none());
    assert_eq!(
        attachment_get.result.expect("attachment get result")["response"]["payload"]["name"],
        json!("sample.txt")
    );

    let attachment_list = daemon
        .handle_rpc(rpc_request(
            1343,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.attachment.list",
                "kind": "query",
                "payload": {
                    "topic_id": topic_id.clone(),
                    "limit": 10,
                },
            }),
        ))
        .expect("attachment list envelope");
    assert!(attachment_list.error.is_none());
    assert_eq!(
        attachment_list.result.expect("attachment list result")["response"]["payload"]
            ["attachments"]
            .as_array()
            .expect("attachments")
            .len(),
        1
    );

    let attachment_associate = daemon
        .handle_rpc(rpc_request(
            1344,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_attachment_associate_topic_v2",
                "kind": "command",
                "payload": {
                    "attachment_id": attachment_id.clone(),
                    "topic_id": topic_id,
                },
            }),
        ))
        .expect("attachment associate envelope");
    assert!(attachment_associate.error.is_none());
    assert_eq!(
        attachment_associate.result.expect("attachment associate result")["response"]["payload"]
            ["accepted"],
        json!(true)
    );

    let attachment_delete = daemon
        .handle_rpc(rpc_request(
            1345,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_attachment_delete_v2",
                "kind": "command",
                "payload": attachment_id,
            }),
        ))
        .expect("attachment delete envelope");
    assert!(attachment_delete.error.is_none());
    assert_eq!(
        attachment_delete.result.expect("attachment delete result")["response"]["payload"]
            ["accepted"],
        json!(true)
    );
}

#[test]
fn sdk_operation_registry_roundtrips_attachment_streaming_family() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"hello world".to_vec();
    let checksum = encode_hex(Sha256::digest(payload.as_slice()));

    let registry = daemon
        .handle_rpc(rpc_request(1346, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let registry_result = registry.result.expect("registry result");
    let entries = registry_result["registry"]["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.attachment.upload_start")));
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.attachment.download_chunk")));

    let upload_start = daemon
        .handle_rpc(rpc_request(
            1347,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_attachment_upload_start_v2",
                "kind": "command",
                "payload": {
                    "name": "chunked.bin",
                    "content_type": "application/octet-stream",
                    "total_size": payload.len(),
                    "checksum_sha256": checksum,
                },
            }),
        ))
        .expect("upload start envelope");
    assert!(upload_start.error.is_none());
    let upload_payload = &upload_start.result.expect("upload start result")["response"]["payload"];
    let upload_id = upload_payload["upload_id"].as_str().expect("upload id").to_string();
    let attachment_id =
        upload_payload["attachment_id"].as_str().expect("attachment id").to_string();

    let upload_chunk = daemon
        .handle_rpc(rpc_request(
            1348,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.attachment.upload_chunk",
                "kind": "command",
                "payload": {
                    "upload_id": upload_id.clone(),
                    "offset": 0,
                    "bytes_base64": "aGVsbG8gd29ybGQ=",
                },
            }),
        ))
        .expect("upload chunk envelope");
    assert!(upload_chunk.error.is_none());
    assert_eq!(
        upload_chunk.result.expect("upload chunk result")["response"]["payload"]["complete"],
        json!(true)
    );

    let upload_commit = daemon
        .handle_rpc(rpc_request(
            1349,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_attachment_upload_commit_v2",
                "kind": "command",
                "payload": {
                    "upload_id": upload_id,
                },
            }),
        ))
        .expect("upload commit envelope");
    assert!(upload_commit.error.is_none());
    assert_eq!(
        upload_commit.result.expect("upload commit result")["response"]["payload"]["attachment_id"],
        json!(attachment_id)
    );

    let download_chunk = daemon
        .handle_rpc(rpc_request(
            1350,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_attachment_download_chunk_v2",
                "kind": "query",
                "payload": {
                    "attachment_id": attachment_id,
                    "offset": 0,
                    "max_bytes": 5,
                },
            }),
        ))
        .expect("download chunk envelope");
    assert!(download_chunk.error.is_none());
    let download_payload =
        &download_chunk.result.expect("download chunk result")["response"]["payload"];
    assert_eq!(download_payload["offset"], json!(0));
    assert_eq!(download_payload["next_offset"], json!(5));
    assert_eq!(download_payload["done"], json!(false));
}

#[test]
fn sdk_operation_registry_roundtrips_marker_family() {
    let daemon = RpcDaemon::test_instance();

    let topic = daemon
        .handle_rpc(rpc_request(
            1340,
            "sdk_topic_create_v2",
            json!({ "topic_path": "ops/markers" }),
        ))
        .expect("topic create");
    let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
        .as_str()
        .expect("topic id")
        .to_string();

    let registry = daemon
        .handle_rpc(rpc_request(1341, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    let registry_result = registry.result.expect("registry result");
    let entries = registry_result["registry"]["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.marker.create")));
    assert!(entries.iter().any(|entry| entry["id"] == json!("app.marker.delete")));

    let marker_create = daemon
        .handle_rpc(rpc_request(
            1342,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_marker_create_v2",
                "kind": "command",
                "payload": {
                    "label": "Alpha",
                    "position": { "lat": 35.0, "lon": -115.0, "alt_m": 1200.0 },
                    "topic_id": topic_id.clone(),
                },
            }),
        ))
        .expect("marker create envelope");
    assert!(marker_create.error.is_none());
    let marker_payload =
        &marker_create.result.expect("marker create result")["response"]["payload"];
    let marker_id = marker_payload["marker_id"].as_str().expect("marker id").to_string();
    let revision = marker_payload["revision"].as_u64().expect("marker revision");

    let marker_list = daemon
        .handle_rpc(rpc_request(
            1343,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.marker.list",
                "kind": "query",
                "payload": {
                    "topic_id": topic_id,
                    "limit": 10,
                },
            }),
        ))
        .expect("marker list envelope");
    assert!(marker_list.error.is_none());
    assert!(!marker_list.result.expect("marker list result")["response"]["payload"]["markers"]
        .as_array()
        .expect("markers")
        .is_empty());

    let marker_update = daemon
        .handle_rpc(rpc_request(
            1344,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.marker.update_position",
                "kind": "command",
                "payload": {
                    "marker_id": marker_id.clone(),
                    "expected_revision": revision,
                    "position": { "lat": 36.0, "lon": -116.0, "alt_m": null },
                },
            }),
        ))
        .expect("marker update envelope");
    assert!(marker_update.error.is_none());
    let updated_revision = marker_update.result.expect("marker update result")["response"]
        ["payload"]["revision"]
        .as_u64()
        .expect("updated revision");

    let marker_delete = daemon
        .handle_rpc(rpc_request(
            1345,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_marker_delete_v2",
                "kind": "command",
                "payload": {
                    "marker_id": marker_id,
                    "expected_revision": updated_revision,
                },
            }),
        ))
        .expect("marker delete envelope");
    assert!(marker_delete.error.is_none());
    assert_eq!(
        marker_delete.result.expect("marker delete result")["response"]["payload"]["accepted"],
        json!(true)
    );
}
