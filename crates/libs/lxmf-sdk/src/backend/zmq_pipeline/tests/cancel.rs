use super::*;

#[test]
fn cancel_uses_zmq_sdk_method_and_decodes_result() {
    let _guard = zmq_cancel_test_guard();
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "message_id": "msg-cancel", "result": "Accepted" }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client.cancel(MessageId("msg-cancel".to_owned())).expect("cancel");

    assert_eq!(result, CancelResult::Accepted);
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_cancel_message_v2");
    assert_eq!(request.params.as_ref().expect("params")["message_id"], json!("msg-cancel"));
    server.join().expect("server joined");
}

#[test]
fn cancel_decodes_all_zmq_contract_result_variants() {
    let _guard = zmq_cancel_test_guard();
    let variants = [
        ("Accepted", CancelResult::Accepted),
        ("AlreadyTerminal", CancelResult::AlreadyTerminal),
        ("NotFound", CancelResult::NotFound),
        ("TooLateToCancel", CancelResult::TooLateToCancel),
    ];
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let responses = variants
        .iter()
        .map(|(variant, _)| {
            json!({
                "message_id": format!("msg-{variant}"),
                "result": variant
            })
        })
        .collect();
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        responses,
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    for (variant, expected) in variants.iter() {
        let message_id = MessageId(format!("msg-{variant}"));
        let result = client.cancel(message_id).expect("cancel variant");
        assert_eq!(result, expected.clone());
    }

    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured.len(), variants.len());
    for (request, (variant, _)) in captured.iter().zip(variants.iter()) {
        assert_eq!(request.method, "sdk_cancel_message_v2");
        assert_eq!(
            request.params.as_ref().expect("params")["message_id"],
            json!(format!("msg-{variant}"))
        );
    }
    server.join().expect("server joined");
}

#[test]
fn envelope_execute_uses_zmq_sdk_method_and_preserves_cancel_result() {
    let _guard = zmq_cancel_test_guard();
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.delivery.cancel",
                "kind": "result",
                "accepted": true,
                "correlation_id": "cancel-corr",
                "payload": {
                    "message_id": "msg-cancel",
                    "result": "Accepted"
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let response = client
        .envelope_execute(
            crate::app::Envelope::command(
                "app.delivery.cancel",
                json!({
                    "message_id": "msg-cancel"
                }),
            )
            .with_correlation_id("cancel-corr"),
        )
        .expect("cancel envelope");

    assert_eq!(response.operation_id.as_str(), "app.delivery.cancel");
    assert!(response.accepted);
    assert_eq!(response.correlation_id.as_deref(), Some("cancel-corr"));
    assert_eq!(response.payload["message_id"], json!("msg-cancel"));
    assert_eq!(response.payload["result"], json!("Accepted"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["operation_id"], json!("app.delivery.cancel"));
    assert_eq!(params["kind"], json!("command"));
    assert_eq!(params["correlation_id"], json!("cancel-corr"));
    assert_eq!(params["payload"]["message_id"], json!("msg-cancel"));
    server.join().expect("server joined");
}

#[test]
fn envelope_execute_preserves_deferred_cancel_result_payload() {
    let _guard = zmq_cancel_test_guard();
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.delivery.cancel",
                "kind": "result",
                "accepted": false,
                "correlation_id": "cancel-deferred-corr",
                "payload": {
                    "message_id": "msg-deferred-cancel",
                    "result": "TooLateToCancel",
                    "queue_state": "deferred"
                },
                "extensions": {
                    "cancel_stage": "deferred"
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let response = client
        .envelope_execute(
            crate::app::Envelope::command(
                "app.delivery.cancel",
                json!({
                    "message_id": "msg-deferred-cancel",
                    "queue_state": "deferred"
                }),
            )
            .with_correlation_id("cancel-deferred-corr"),
        )
        .expect("deferred cancel envelope");

    assert_eq!(response.operation_id.as_str(), "app.delivery.cancel");
    assert!(!response.accepted);
    assert_eq!(response.correlation_id.as_deref(), Some("cancel-deferred-corr"));
    assert_eq!(response.payload["message_id"], json!("msg-deferred-cancel"));
    assert_eq!(response.payload["result"], json!("TooLateToCancel"));
    assert_eq!(response.payload["queue_state"], json!("deferred"));
    assert_eq!(response.extensions["cancel_stage"], json!("deferred"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["operation_id"], json!("app.delivery.cancel"));
    assert_eq!(params["kind"], json!("command"));
    assert_eq!(params["correlation_id"], json!("cancel-deferred-corr"));
    assert_eq!(params["payload"]["message_id"], json!("msg-deferred-cancel"));
    assert_eq!(params["payload"]["queue_state"], json!("deferred"));
    server.join().expect("server joined");
}
