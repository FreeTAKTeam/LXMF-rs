use super::*;

#[test]
fn send_batch_uses_zmq_sdk_method_and_decodes_ordered_results() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "batch_id": "batch-typed-1",
            "accepted_count": 1,
            "rejected_count": 1,
            "results": [
                {
                    "id": "batch-msg-1",
                    "message_id": "batch-msg-1",
                    "accepted": true
                },
                {
                    "id": "batch-msg-2",
                    "accepted": false,
                    "error": {
                        "code": "SDK_QUEUE_FULL",
                        "message": "outbound delivery queue full",
                        "category": "Runtime",
                        "retryable": true
                    }
                }
            ]
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .send_batch(crate::BatchSendRequest {
            batch_id: "batch-typed-1".to_string(),
            source: "source-destination".to_string(),
            messages: vec![
                crate::BatchSendItem::new(
                    "batch-msg-1",
                    "peer-a",
                    json!({
                        "title": "hello a",
                        "body": "payload a https://example.invalid/a",
                        "FIELD_THREAD": "thread-a"
                    }),
                )
                .with_delivery_method("direct")
                .with_idempotency_key("batch-msg-1-send-once")
                .with_ttl_ms(30_000)
                .with_correlation_id("batch-msg-1-corr")
                .with_extension("burst_slot", json!(0))
                .with_include_ticket(false),
                crate::BatchSendItem::new(
                    "batch-msg-2",
                    "peer-b",
                    json!({
                        "title": "hello b",
                        "content": "payload b",
                        "FIELD_GROUP": "group-b"
                    }),
                )
                .with_try_propagation_on_fail(true)
                .with_stamp_cost(12),
            ],
        })
        .expect("batch send");

    assert_eq!(result.batch_id, "batch-typed-1");
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.results[0].message_id.as_deref(), Some("batch-msg-1"));
    assert!(result.results[0].accepted);
    assert!(!result.results[1].accepted);
    assert_eq!(result.results[1].error.as_ref().expect("error").code, "SDK_QUEUE_FULL");
    assert!(result.results[1].error.as_ref().expect("error").retryable);

    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_send_batch_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["batch_id"], json!("batch-typed-1"));
    assert_eq!(params["source"], json!("source-destination"));
    assert_eq!(params["messages"][0]["id"], json!("batch-msg-1"));
    assert_eq!(params["messages"][0]["destination"], json!("peer-a"));
    assert_eq!(params["messages"][0]["title"], json!("hello a"));
    assert_eq!(params["messages"][0]["content"], json!("payload a https://example.invalid/a"));
    assert_eq!(params["messages"][0]["fields"]["FIELD_THREAD"], json!("thread-a"));
    assert_eq!(params["messages"][0]["fields"].get("body"), None);
    assert_eq!(params["messages"][0]["fields"].get("title"), None);
    assert_eq!(params["messages"][0]["fields"].get("_sdk"), None);
    assert_eq!(params["messages"][0]["method"], json!("direct"));
    assert_eq!(params["messages"][0]["include_ticket"], json!(false));
    assert_eq!(params["messages"][0]["idempotency_key"], json!("batch-msg-1-send-once"));
    assert_eq!(params["messages"][0]["ttl_ms"], json!(30_000));
    assert_eq!(params["messages"][0]["correlation_id"], json!("batch-msg-1-corr"));
    assert_eq!(params["messages"][0]["extensions"]["burst_slot"], json!(0));
    assert_eq!(params["messages"][1]["fields"]["FIELD_GROUP"], json!("group-b"));
    assert_eq!(params["messages"][1]["fields"].get("content"), None);
    assert_eq!(params["messages"][1]["fields"].get("title"), None);
    assert_eq!(params["messages"][1]["try_propagation_on_fail"], json!(true));
    assert_eq!(params["messages"][1]["stamp_cost"], json!(12));
    server.join().expect("server joined");
}
