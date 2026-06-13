use super::*;

#[test]
fn local_delivery_destination_hash_uses_zmq_sdk_status_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "identity_hash": "identity-destination",
            "delivery_destination_hash": "delivery-destination"
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let destination_hash =
        client.local_delivery_destination_hash().expect("delivery destination hash");

    assert_eq!(destination_hash, "delivery-destination");
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "status");
    assert_eq!(request.params, Some(json!({})));
    server.join().expect("server joined");
}
