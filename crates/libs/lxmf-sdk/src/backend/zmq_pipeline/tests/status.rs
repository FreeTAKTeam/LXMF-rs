use super::*;

#[test]
fn status_treats_sent_as_terminal_until_receipt_terminality_is_negotiated() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "message": {
                "message_id": "msg-sent",
                "receipt_status": "sent",
                "timestamp": 1710000000
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let snapshot =
        client.status(MessageId("msg-sent".to_owned())).expect("status").expect("message");

    assert_eq!(snapshot.state, DeliveryState::Sent);
    assert!(snapshot.terminal);
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_status_v2");
    assert_eq!(request.params.as_ref().expect("params")["message_id"], json!("msg-sent"));
    server.join().expect("server joined");
}

#[test]
fn status_keeps_sent_nonterminal_after_receipt_terminality_is_negotiated() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "runtime_id": "runtime-zmq-receipts",
                "active_contract_version": 2,
                "effective_capabilities": ["sdk.capability.receipt_terminality"],
                "effective_limits": {
                    "max_poll_events": 64,
                    "max_event_bytes": 32768,
                    "max_batch_bytes": 1048576,
                    "max_extension_keys": 32,
                    "idempotency_ttl_ms": 60000
                },
                "contract_release": "v2",
                "schema_namespace": "sdk.v2"
            }),
            json!({
                "message": {
                    "message_id": "msg-sent",
                    "receipt_status": "sent",
                    "timestamp": 1710000000
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    client
        .negotiate(crate::capability::NegotiationRequest {
            supported_contract_versions: vec![2],
            requested_capabilities: vec!["sdk.capability.receipt_terminality".to_owned()],
            profile: crate::types::Profile::DesktopLocalRuntime,
            bind_mode: crate::types::BindMode::LocalOnly,
            auth_mode: crate::types::AuthMode::LocalTrusted,
            overflow_policy: crate::types::OverflowPolicy::Reject,
            block_timeout_ms: None,
            rpc_backend: None,
            extensions: Default::default(),
        })
        .expect("negotiate");
    let snapshot =
        client.status(MessageId("msg-sent".to_owned())).expect("status").expect("message");

    assert_eq!(snapshot.state, DeliveryState::Sent);
    assert!(!snapshot.terminal);
    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].method, "sdk_negotiate_v2");
    assert_eq!(captured[1].method, "sdk_status_v2");
    server.join().expect("server joined");
}

#[test]
fn status_preserves_retry_attempts_and_reason_code_from_zmq_sdk_response() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "message": {
                "message_id": "msg-retry",
                "receipt_status": "failed: no path",
                "timestamp": 1710000100,
                "attempts": 3,
                "reason_code": "no_path"
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let snapshot =
        client.status(MessageId("msg-retry".to_owned())).expect("status").expect("message");

    assert_eq!(snapshot.state, DeliveryState::Failed);
    assert!(snapshot.terminal);
    assert_eq!(snapshot.attempts, 3);
    assert_eq!(snapshot.reason_code.as_deref(), Some("no_path"));
    assert_eq!(snapshot.last_updated_ms, 1_710_000_100_000);
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_status_v2");
    assert_eq!(request.params.as_ref().expect("params")["message_id"], json!("msg-retry"));
    server.join().expect("server joined");
}

#[test]
fn status_rejects_malformed_retry_metadata_from_zmq_sdk_response() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "message": {
                "message_id": "msg-retry-invalid",
                "receipt_status": "failed: no path",
                "timestamp": 1710000101,
                "attempts": "3",
                "reason_code": 404
            }
        }),
        Arc::new(Mutex::new(None)),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let err = client
        .status(MessageId("msg-retry-invalid".to_owned()))
        .expect_err("malformed retry metadata should fail status decoding");

    assert_eq!(err.category, ErrorCategory::Internal);
    assert!(
        err.message.contains("attempts") || err.message.contains("reason_code"),
        "unexpected error: {err:?}"
    );
    server.join().expect("server joined");
}
