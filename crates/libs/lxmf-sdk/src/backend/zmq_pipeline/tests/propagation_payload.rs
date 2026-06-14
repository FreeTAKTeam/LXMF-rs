use super::*;

#[test]
fn propagation_local_payload_ingest_and_fetch_use_zmq_sdk_envelopes() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "response": {
                    "operation_id": "app.propagation.ingest",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "ingested_count": 1,
                        "duplicate_count": 0,
                        "payload_bytes": 18,
                        "transferred_bytes": 18,
                        "transient_id": "transient-sdk-ingest",
                        "propagation": {
                            "enabled": true,
                            "selected_node": "router-ingest",
                            "sync_state": 1,
                            "state_name": "queued",
                            "queue_depth": 5,
                            "total_ingested": 11,
                            "last_ingest_count": 1
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.fetch",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "transient_id": "transient-sdk-ingest",
                        "payload_hex": "70726f7061676174696f6e2d7061796c6f6164",
                        "payload_bytes": 18,
                        "transferred_bytes": 18,
                        "propagation": {
                            "enabled": true,
                            "selected_node": "router-fetch",
                            "sync_state": 2,
                            "state_name": "serving",
                            "queue_depth": 4,
                            "client_propagation_messages_served": 1
                        }
                    }
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let ingested = client
        .propagation_ingest(crate::PropagationIngestRequest {
            transient_id: Some("transient-sdk-ingest".to_string()),
            payload_hex: Some("70726f7061676174696f6e2d7061796c6f6164".to_string()),
        })
        .expect("propagation ingest");
    let fetched = client
        .propagation_fetch(crate::PropagationFetchRequest {
            transient_id: "transient-sdk-ingest".to_string(),
        })
        .expect("propagation fetch");

    assert_eq!(ingested.ingested_count, 1);
    assert_eq!(ingested.duplicate_count, 0);
    assert_eq!(ingested.payload_bytes, 18);
    assert_eq!(ingested.transient_id, "transient-sdk-ingest");
    assert_eq!(ingested.propagation["selected_node"], json!("router-ingest"));
    assert_eq!(ingested.recovery_state.selected_node.as_deref(), Some("router-ingest"));
    assert_eq!(ingested.recovery_state.sync_state, 1);
    assert_eq!(ingested.recovery_state.state_name.as_deref(), Some("queued"));
    assert_eq!(ingested.recovery_state.queue_depth, 5);
    assert_eq!(ingested.recovery_state.total_ingested, 11);
    assert_eq!(ingested.recovery_state.last_ingest_count, 1);
    assert_eq!(fetched.transient_id, "transient-sdk-ingest");
    assert_eq!(fetched.payload_hex, "70726f7061676174696f6e2d7061796c6f6164");
    assert_eq!(fetched.transferred_bytes, 18);
    assert_eq!(fetched.propagation["selected_node"], json!("router-fetch"));
    assert_eq!(fetched.recovery_state.selected_node.as_deref(), Some("router-fetch"));
    assert_eq!(fetched.recovery_state.sync_state, 2);
    assert_eq!(fetched.recovery_state.state_name.as_deref(), Some("serving"));
    assert_eq!(fetched.recovery_state.queue_depth, 4);
    assert_eq!(fetched.recovery_state.client_propagation_messages_served, 1);

    let captured = captured.lock().expect("captured requests");
    let operation_ids = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params")["operation_id"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        operation_ids,
        vec![json!("app.propagation.ingest"), json!("app.propagation.fetch")]
    );
    let kinds = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params")["kind"].clone())
        .collect::<Vec<_>>();
    assert_eq!(kinds, vec![json!("command"), json!("command")]);
    assert_eq!(
        captured[0].params.as_ref().expect("params")["payload"],
        json!({
            "transient_id": "transient-sdk-ingest",
            "payload_hex": "70726f7061676174696f6e2d7061796c6f6164"
        })
    );
    assert_eq!(
        captured[1].params.as_ref().expect("params")["payload"],
        json!({ "transient_id": "transient-sdk-ingest" })
    );
    server.join().expect("server joined");
}
