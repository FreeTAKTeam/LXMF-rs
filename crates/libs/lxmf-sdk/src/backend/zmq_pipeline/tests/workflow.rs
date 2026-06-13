use super::*;

#[test]
fn workflow_peer_ready_uses_zmq_sdk_method_and_preserves_contact_metadata() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "workflow": {
                "identity": "peer-ready",
                "contact": {
                    "identity": "peer-ready",
                    "display_name": "RCH Relay",
                    "trust_level": "trusted",
                    "bootstrap": true,
                    "updated_ts_ms": 1700000400,
                    "metadata": {
                        "callsign": "RCH-1",
                        "capabilities": ["rem.direct_chat", "rch.announce_slot"]
                    },
                    "extensions": {
                        "source": "zmq"
                    }
                },
                "was_created": true,
                "announced": true
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .workflow_peer_ready(crate::WorkflowPeerReadyRequest {
            identity: crate::IdentityRef("peer-ready".to_owned()),
            display_name: Some("RCH Relay".to_owned()),
            trust_level: Some(crate::TrustLevel::Trusted),
            bootstrap: Some(true),
            announce: Some(true),
            metadata: BTreeMap::from([
                ("callsign".to_owned(), json!("RCH-1")),
                ("capabilities".to_owned(), json!(["rem.direct_chat", "rch.announce_slot"])),
            ]),
            extensions: BTreeMap::from([("source".to_owned(), json!("rem-rch"))]),
        })
        .expect("workflow peer ready");

    assert_eq!(result.identity.0, "peer-ready");
    assert_eq!(result.contact.display_name.as_deref(), Some("RCH Relay"));
    assert_eq!(result.contact.metadata["callsign"], json!("RCH-1"));
    assert_eq!(result.contact.metadata["capabilities"][1], json!("rch.announce_slot"));
    assert!(result.was_created);
    assert!(result.announced);

    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_workflow_peer_ready_v2");
    let params = request.params.as_ref().expect("params");
    assert_eq!(params["identity"], json!("peer-ready"));
    assert_eq!(params["display_name"], json!("RCH Relay"));
    assert_eq!(params["trust_level"], json!("trusted"));
    assert_eq!(params["bootstrap"], json!(true));
    assert_eq!(params["announce"], json!(true));
    assert_eq!(params["metadata"]["callsign"], json!("RCH-1"));
    assert_eq!(params["metadata"]["capabilities"][0], json!("rem.direct_chat"));
    assert_eq!(params["extensions"]["source"], json!("rem-rch"));
    server.join().expect("server joined");
}
