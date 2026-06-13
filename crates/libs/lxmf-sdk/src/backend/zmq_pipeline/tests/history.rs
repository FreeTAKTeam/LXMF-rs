use super::*;

#[test]
fn list_message_history_uses_zmq_sdk_method_and_preserves_receipts_and_fields() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.message.history.list",
                "kind": "result",
                "accepted": true,
                "correlation_id": null,
                "payload": {
                    "messages": [{
                        "id": "msg-history-1",
                        "source": "local-destination",
                        "destination": "peer-destination",
                        "title": "chat",
                        "content": "see https://example.invalid/status",
                        "timestamp": 1_700_000_111,
                        "direction": "outbound",
                        "fields": {
                            "FIELD_THREAD": "thread-1",
                            "renderer": { "kind": "plain" }
                        },
                        "receipt_status": "delivered"
                    }],
                    "next_cursor": "1700000111:msg-history-1"
                },
                "extensions": {
                    "restart_recovery": true
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let page = client
        .list_message_history(crate::MessageHistoryListRequest {
            peer_id: Some("peer-destination".to_string()),
            conversation_id: Some("peer-destination".to_string()),
            include_receipts: Some(true),
            limit: Some(25),
            cursor: Some("1700000120:msg-history-2".to_string()),
            before_ts: None,
        })
        .expect("history page");

    assert_eq!(page.next_cursor.as_deref(), Some("1700000111:msg-history-1"));
    assert_eq!(page.messages.len(), 1);
    let message = &page.messages[0];
    assert_eq!(message.id, "msg-history-1");
    assert_eq!(message.content, "see https://example.invalid/status");
    assert_eq!(message.receipt_status.as_deref(), Some("delivered"));
    assert_eq!(message.fields.as_ref().expect("fields")["FIELD_THREAD"], json!("thread-1"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    assert_eq!(
        request.params,
        Some(json!({
            "operation_id": "app.message.history.list",
            "kind": "query",
            "target": null,
            "correlation_id": null,
            "timeout_ms": null,
            "payload": {
                "peer_id": "peer-destination",
                "conversation_id": "peer-destination",
                "include_receipts": true,
                "limit": 25,
                "cursor": "1700000120:msg-history-2"
            },
            "extensions": {}
        }))
    );
    server.join().expect("server joined");
}
