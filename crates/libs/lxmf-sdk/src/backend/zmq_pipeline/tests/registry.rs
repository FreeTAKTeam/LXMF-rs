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
    ] {
        assert!(registry.supports(operation_id), "{operation_id}");
    }
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_operation_registry_v2");
    assert_eq!(request.params, Some(json!({})));
    server.join().expect("server joined");
}
