use super::*;
use std::collections::BTreeMap;

#[test]
fn identity_create_uses_zmq_sdk_method_and_returns_delivery_destination() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "identity": {
                "identity": "service-identity",
                "delivery_destination": "service-delivery",
                "public_key": "pubkey",
                "display_name": "Service",
                "capabilities": ["lxmf.delivery"],
                "metadata": {"service": "test"},
                "extensions": {}
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let identity = client
        .identity_create(crate::domain::IdentityCreateRequest {
            display_name: Some("Service".to_string()),
            capabilities: vec!["lxmf.delivery".to_string()],
            metadata: BTreeMap::from([("service".to_string(), json!("test"))]),
            extensions: BTreeMap::new(),
        })
        .expect("identity create");

    assert_eq!(identity.identity.0, "service-identity");
    assert_eq!(identity.delivery_destination.as_deref(), Some("service-delivery"));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_create_v2");
    assert_eq!(request.params.as_ref().expect("params")["display_name"], "Service");
    server.join().expect("server joined");
}
