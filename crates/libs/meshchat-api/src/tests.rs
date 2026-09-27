use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

async fn call(app: Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let request = request
        .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
        .expect("request");
    let response = app.oneshot(request).await.expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("response bytes");
    let value = serde_json::from_slice(&bytes).expect("json response");
    (status, value)
}

#[tokio::test]
async fn propagation_sync_requires_selected_node() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let (status, result) = call(app, "GET", "/api/v1/lxmf/propagation-node/sync", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(result["message"].as_str().unwrap_or_default().contains("must be configured"));
}

#[tokio::test]
async fn preferred_propagation_node_config_validates_and_selects() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let key = "lxmf_preferred_propagation_node_destination_hash";
    let (status, _) = call(
        app.clone(),
        "PATCH",
        "/api/v1/config",
        Some(json!({
            "config":{key:"Beleth LXMD PN"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        app.clone(),
        "PATCH",
        "/api/v1/config",
        Some(json!({
            "config":{key:"0123456789abcdef0123456789abcdef", "unsupported_setting":true}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let peer = "0123456789abcdef0123456789ABCDEF";
    let (status, response) = call(
        app,
        "PATCH",
        "/api/v1/config",
        Some(json!({
            "config":{key:peer}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["config"][key], peer.to_ascii_lowercase());
}

#[tokio::test]
async fn text_message_roundtrip_uses_daemon_store_and_meshchat_shape() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let destination = "fedcba9876543210fedcba9876543210";
    let app = router(daemon, None);
    let (status, sent) = call(
        app.clone(),
        "POST",
        "/api/v1/lxmf-messages/send",
        Some(json!({
            "delivery_method":null,
            "lxmf_message":{"destination_hash":destination,"content":"hello mesh"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent["lxmf_message"]["content"], "hello mesh");
    assert_eq!(sent["lxmf_message"]["hash"].as_str().expect("hash").len(), 64);
    let (status, history) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/lxmf-messages/conversation/{destination}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let messages = history["lxmf_messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["hash"], sent["lxmf_message"]["hash"]);
    assert_eq!(messages[0]["content"], "hello mesh");
    let hash = sent["lxmf_message"]["hash"].as_str().expect("hash");
    let (status, deleted) =
        call(app.clone(), "DELETE", &format!("/api/v1/lxmf-messages/{hash}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["message"], "ok");
    let (_, history) =
        call(app, "GET", &format!("/api/v1/lxmf-messages/conversation/{destination}"), None).await;
    assert!(history["lxmf_messages"].as_array().expect("messages").is_empty());
}

#[tokio::test]
async fn unsupported_attachment_send_is_explicit_and_does_not_store_message() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let app = router(daemon, None);
    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/lxmf-messages/send",
        Some(json!({
            "lxmf_message":{"destination_hash":"fedcba9876543210fedcba9876543210",
                "content":"","fields":{"audio":{"audio_mode":"x","audio_bytes":"AA=="}}}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let (_, conversations) = call(app, "GET", "/api/v1/lxmf/conversations", None).await;
    assert!(conversations["conversations"].as_array().expect("conversations").is_empty());
}

#[tokio::test]
async fn file_send_roundtrips_through_daemon_history() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let destination = "fedcba9876543210fedcba9876543210";
    let files = json!([{"file_name":"hello.txt","file_bytes":"aGVsbG8="}]);
    let (status, sent) = call(
        app.clone(),
        "POST",
        "/api/v1/lxmf-messages/send",
        Some(json!({
            "lxmf_message":{"destination_hash":destination,"content":"file test",
                "fields":{"file_attachments":files}}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent["lxmf_message"]["fields"]["file_attachments"], files);
    let (status, history) =
        call(app, "GET", &format!("/api/v1/lxmf-messages/conversation/{destination}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["lxmf_messages"][0]["fields"]["file_attachments"], files);
}

#[test]
fn inbound_wire_file_field_maps_to_meshchat_download() {
    let record = json!({"direction":"in","fields":{"5":[["hello.txt",[104,101,108,108,111]]]}});
    let message = chat::stored_message(&record);
    assert_eq!(
        message["fields"]["file_attachments"],
        json!([{"file_name":"hello.txt","file_bytes":"aGVsbG8="}])
    );
}

#[test]
fn inbound_wire_image_field_maps_to_meshchat_display() {
    let record = json!({"direction":"in","content":"Test","fields":{"6":["jpg",[255,216,255]]}});
    let message = chat::stored_message(&record);
    assert_eq!(message["content"], "Test");
    assert_eq!(message["fields"]["image"], json!({"image_type":"jpg","image_bytes":"/9j/"}));
}

#[test]
fn inbound_wire_audio_field_maps_to_meshchat_playback() {
    let record = json!({"direction":"in","content":"audio","fields":{"7":[16,[79,103,103,83]]}});
    let message = chat::stored_message(&record);
    assert_eq!(
        message["fields"]["audio"],
        json!({
            "audio_mode":16,"audio_bytes":"T2dnUw=="
        })
    );
}

#[test]
fn inbound_sideband_time_telemetry_is_exposed_without_losing_raw_data() {
    let record = json!({"direction":"in","content":"","fields":{"2":[129,1,206,106,185,39,67]}});
    let message = chat::stored_message(&record);
    assert_eq!(message["fields"]["telemetry"]["utc"], 1_790_519_107);
    assert_eq!(message["fields"]["telemetry"]["sensor_ids"], json!([1]));
    assert_eq!(message["fields"]["telemetry"]["raw_bytes"], "gQHOarknQw==");
}

#[test]
fn inbound_sideband_battery_and_missing_position_are_reported() {
    let bytes = hex::decode("8301ce6ab9337502c00493cb4058800000000000c2c0")
        .expect("captured Sideband telemetry");
    let message = chat::stored_message(&json!({"direction":"in","fields":{"2":bytes}}));
    let telemetry = &message["fields"]["telemetry"];
    assert_eq!(telemetry["sensor_ids"], json!([1, 2, 4]));
    assert_eq!(telemetry["position"], Value::Null);
    assert_eq!(telemetry["battery"], json!({"charge_percent":98.0,"charging":false}));
}

#[test]
fn inbound_sideband_position_fix_decodes_coordinates() {
    let position = rmpv::Value::Array(vec![
        rmpv::Value::Binary(45_123_456_i32.to_be_bytes().to_vec()),
        rmpv::Value::Binary((-63_123_456_i32).to_be_bytes().to_vec()),
        rmpv::Value::Binary(0_i32.to_be_bytes().to_vec()),
        rmpv::Value::Binary(0_u32.to_be_bytes().to_vec()),
        rmpv::Value::Binary(0_i32.to_be_bytes().to_vec()),
        rmpv::Value::Binary(250_u16.to_be_bytes().to_vec()),
        rmpv::Value::from(1_790_522_229_i64),
    ]);
    let packet = rmpv::Value::Map(vec![(rmpv::Value::from(2), position)]);
    let mut bytes = Vec::new();
    rmpv::encode::write_value(&mut bytes, &packet).expect("encode telemetry");
    let message = chat::stored_message(&json!({"direction":"in","fields":{"2":bytes}}));
    assert_eq!(
        message["fields"]["telemetry"]["position"],
        json!({
            "latitude":45.123456,"longitude":-63.123456,"accuracy":2.5,"updated":1_790_522_229
        })
    );
}

#[test]
fn command_only_messages_are_hidden_without_hiding_user_content() {
    let request = json!({"direction":"in","title":"","content":"",
        "fields":{"9":[{"1":[1_789_916_588.0,false]}],"_lxmf":{"signature_valid":true}}});
    assert!(chat::is_command_only_message(&request));
    let with_ticket = json!({"title":"","content":"","fields":{"9":[{"1":[1.0,false]}],"12":[1]}});
    assert!(chat::is_command_only_message(&with_ticket));
    let other_command = json!({"title":"","content":"","fields":{"9":[{"0":"command"}]}});
    assert!(chat::is_command_only_message(&other_command));
    let with_content = json!({"title":"","content":"hello","fields":{"9":[{"1":[1.0,false]}]}});
    assert!(!chat::is_command_only_message(&with_content));
    let with_telemetry =
        json!({"title":"","content":"","fields":{"9":[{"1":[1.0,false]}],"2":{"lat":1}}});
    assert!(!chat::is_command_only_message(&with_telemetry));
}

#[tokio::test]
async fn conversation_page_skips_stored_telemetry_requests() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let source = "52118fa6f1240342cb730dabdf1d4ca1";
    for (id, content, fields) in
        [("text", "hello", None), ("request", "", Some(json!({"9":[{"1":[1.0,false]}]})))]
    {
        daemon
            .accept_inbound(rns_rpc::MessageRecord {
                id: id.into(),
                source: source.into(),
                destination: "19abf5a03e0beb386ed605dbcca1ea75".into(),
                title: String::new(),
                content: content.into(),
                timestamp: 1,
                direction: "in".into(),
                fields,
                receipt_status: None,
            })
            .expect("store inbound message");
    }
    let app = router(daemon, None);
    let (status, response) = call(
        app,
        "GET",
        &format!("/api/v1/lxmf-messages/conversation/{source}?count=1&order=desc"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["lxmf_messages"].as_array().expect("messages").len(), 1);
    assert_eq!(response["lxmf_messages"][0]["content"], "hello");
}

#[test]
fn failed_resource_send_keeps_the_daemon_reason() {
    let record = json!({"direction":"out","receipt_status":"failed: resource transfer timed out"});
    let message = chat::stored_message(&record);
    assert_eq!(message["state"], "failed");
    assert_eq!(message["failure_reason"], "resource transfer timed out");
}

#[tokio::test]
async fn image_send_roundtrips_through_daemon_history() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let destination = "fedcba9876543210fedcba9876543210";
    let image = json!({"image_type":"jpg","image_bytes":"/9j/"});
    let (status, sent) = call(app.clone(), "POST", "/api/v1/lxmf-messages/send", Some(json!({
        "lxmf_message":{"destination_hash":destination,"content":"image test", "fields":{"image":image}}
    }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent["lxmf_message"]["fields"]["image"], image);
    let (status, history) =
        call(app, "GET", &format!("/api/v1/lxmf-messages/conversation/{destination}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["lxmf_messages"][0]["fields"]["image"], image);
}

#[tokio::test]
async fn invalid_file_base64_is_rejected_before_storage() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/lxmf-messages/send",
        Some(json!({
            "lxmf_message":{"destination_hash":"fedcba9876543210fedcba9876543210",
                "content":"","fields":{"file_attachments":[{"file_name":"x","file_bytes":"!!"}]}}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, conversations) = call(app, "GET", "/api/v1/lxmf/conversations", None).await;
    assert!(conversations["conversations"].as_array().expect("conversations").is_empty());
}

#[tokio::test]
async fn interface_list_exposes_read_only_status() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![
        rns_rpc::rpc::InterfaceRecord {
            kind: "tcp_client".into(),
            enabled: true,
            host: Some("rmap.world".into()),
            port: Some(4242),
            name: Some("meshchat-tcp".into()),
            settings: None,
        },
        rns_rpc::rpc::InterfaceRecord {
            kind: "lora".into(),
            enabled: true,
            host: None,
            port: None,
            name: Some("meshchat-rnode".into()),
            settings: Some(json!({"_runtime":{"lora":{"rnode_status":{"online":true}}}})),
        },
    ]);
    let app = router(daemon, None);
    let (status, body) = call(app, "GET", "/api/v1/reticulum/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["interfaces"][0]["name"], "meshchat-tcp");
    assert_eq!(body["interfaces"][0]["type"], "tcp_client");
    assert_eq!(body["interfaces"][0]["enabled"], true);
    assert_eq!(body["interfaces"][1]["connection"], "online");
}

#[tokio::test]
async fn announce_updates_profile_timestamp() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let (status, _) = call(app.clone(), "GET", "/api/v1/announce", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, value) = call(app, "GET", "/api/v1/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["config"]["last_announced_at"].as_u64().is_some());
}

#[tokio::test]
async fn config_patch_rejects_daemon_keys_without_claiming_success() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let app = router(daemon, None);
    let (status, _) = call(
        app.clone(),
        "PATCH",
        "/api/v1/config",
        Some(json!({
            "config":{"lxmf_local_propagation_node_enabled":true}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let (status, value) = call(app, "GET", "/api/v1/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["config"]["lxmf_local_propagation_node_enabled"], false);
}

#[tokio::test]
async fn frontend_conversation_reads_have_contract_envelopes_without_a_transport() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let destination = "fedcba9876543210fedcba9876543210";
    let (status, path) =
        call(app.clone(), "GET", &format!("/api/v1/destination/{destination}/path"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(path["path"], Value::Null);

    let (status, stamp) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/destination/{destination}/lxmf-stamp-info"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(stamp["lxmf_stamp_info"]["stamp_cost"].is_null());

    let (status, signal) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/destination/{destination}/signal-metrics"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(signal["signal_metrics"]["updated_at"].is_null());

    let (status, calls) = call(app.clone(), "GET", "/api/v1/calls", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(calls["audio_calls"], json!([]));

    let (status, propagation) =
        call(app, "GET", "/api/v1/lxmf/propagation-node/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(propagation["propagation_node_status"].is_object());
}

#[tokio::test]
async fn conversation_history_uses_stable_numeric_cursor_for_older_messages() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    let destination = "fedcba9876543210fedcba9876543210";
    for content in ["first", "second"] {
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/lxmf-messages/send",
            Some(json!({"lxmf_message":{"destination_hash":destination,"content":content}})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let path = format!("/api/v1/lxmf-messages/conversation/{destination}");
    let (status, first_page) =
        call(app.clone(), "GET", &format!("{path}?order=desc&count=1"), None).await;
    assert_eq!(status, StatusCode::OK);
    let newest = &first_page["lxmf_messages"][0];
    assert_eq!(newest["content"], "second");
    let cursor = newest["id"].as_i64().expect("numeric row id");
    let (status, next_page) =
        call(app, "GET", &format!("{path}?order=desc&count=1&after_id={cursor}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next_page["lxmf_messages"][0]["content"], "first");
    assert!(next_page["lxmf_messages"][0]["id"].as_i64().expect("row id") < cursor);
}
