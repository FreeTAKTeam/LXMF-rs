use super::*;

#[test]
fn operation_registry_uses_zmq_sdk_method_for_chat_peer_and_propagation_operations() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "registry": crate::app::OperationRegistry::built_in() }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");
    let registry = client.operation_registry().expect("operation registry");

    for operation_id in [
        "app.message.conversation.list",
        "app.message.history.list",
        "app.delivery.destination_hash",
        "app.delivery.cancel",
        "app.peer.connect",
        "app.peer.disconnect",
        "app.peer.reconnect",
        "app.propagation.peer_sync",
        "app.propagation.remote_status",
        "app.propagation.remote_fetch",
        "app.propagation.remote_download",
        "app.propagation.remote_sync",
        "app.propagation.remote_unpeer",
        "app.propagation.acknowledge_sync_completion",
        "app.propagation.node.get",
        "app.propagation.node.set",
        "app.propagation.node.list",
        "app.propagation.status",
        "app.propagation.enable",
        "app.propagation.delivery_policy.get",
        "app.propagation.delivery_policy.set",
        "app.propagation.peer_maintenance",
        "app.propagation.ingest",
        "app.propagation.fetch",
        "app.paper.encode",
        "app.paper.decode",
    ] {
        assert!(registry.supports(operation_id), "{operation_id}");
    }
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_operation_registry_v2");
    assert_eq!(request.params, Some(json!({})));
    server.join().expect("server joined");
}

#[test]
fn app_paper_encode_uses_zmq_paper_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "envelope": {
                "uri": "lxm://paper-msg-1",
                "transient_id": "paper-msg-1",
                "destination_hint": "dest"
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let app = crate::app::Client::new(ZmqPipelineBackendClient::new(config).expect("zmq client"));

    let response = app
        .command("app.paper.encode", json!({ "message_id": "paper-msg-1" }))
        .expect("paper encode");

    assert_eq!(response.operation_id.as_str(), "app.paper.encode");
    assert_eq!(response.payload["uri"], json!("lxm://paper-msg-1"));
    assert_eq!(response.payload["transient_id"], json!("paper-msg-1"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_paper_encode_v2");
    assert_eq!(request.params, Some(json!({ "message_id": "paper-msg-1" })));
    server.join().expect("server joined");
}

#[test]
fn paper_decode_uses_zmq_paper_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "accepted": true
            }),
            json!({
                "accepted": true,
                "duplicate": true,
                "transient_id": "paper-msg-1",
                "destination": "dest-hash",
                "destination_hint": "dest-hash",
                "bytes_len": 72
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let ack = client
        .paper_decode(crate::domain::PaperMessageEnvelope {
            uri: "lxm://paper-msg-1".to_owned(),
            transient_id: Some("paper-msg-1".to_owned()),
            destination_hint: Some("dest".to_owned()),
            extensions: BTreeMap::new(),
        })
        .expect("paper decode");

    assert!(ack.accepted);
    let result = client
        .paper_decode_with_metadata(crate::domain::PaperMessageEnvelope {
            uri: "lxm://paper-msg-1".to_owned(),
            transient_id: Some("paper-msg-1".to_owned()),
            destination_hint: Some("dest".to_owned()),
            extensions: BTreeMap::new(),
        })
        .expect("paper decode metadata");

    assert!(result.accepted);
    assert!(result.duplicate);
    assert_eq!(result.transient_id, "paper-msg-1");
    assert_eq!(result.destination, "dest-hash");
    assert_eq!(result.destination_hint, "dest-hash");
    assert_eq!(result.bytes_len, 72);
    assert_eq!(result.ack().accepted, result.accepted);
    let captured = captured.lock().expect("captured request");
    assert_eq!(captured.len(), 2);
    for request in captured.iter() {
        assert_eq!(request.method, "sdk_paper_decode_v2");
        assert_eq!(request.params.as_ref().expect("params")["uri"], json!("lxm://paper-msg-1"));
        assert_eq!(request.params.as_ref().expect("params")["destination_hint"], json!("dest"));
    }
    server.join().expect("server joined");
}
