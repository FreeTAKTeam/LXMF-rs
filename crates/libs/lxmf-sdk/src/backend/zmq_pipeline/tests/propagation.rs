use super::*;

#[test]
fn propagation_peer_sync_uses_zmq_sdk_envelope_and_preserves_queue_state() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.propagation.peer_sync",
                "kind": "result",
                "accepted": true,
                "correlation_id": null,
                "payload": {
                    "peer": "peer-prop-a",
                    "peer_type": "manual",
                    "type": "discovered",
                    "synced": false,
                    "postponed": true,
                    "postpone_reason": "backoff",
                    "last_sync_attempt": 1_700_000_100,
                    "next_sync_attempt": 1_700_000_700,
                    "sync_backoff": 600,
                    "transfer_limit": 42500,
                    "sync_limit": 84000,
                    "messages": {
                        "offered": 2,
                        "outgoing": 1,
                        "incoming": 0,
                        "unhandled": 1,
                        "handled_ids": ["aa"],
                        "unhandled_ids": ["bb"]
                    },
                    "propagation": {
                        "synced": false,
                        "postponed": true,
                        "postpone_reason": "backoff",
                        "offered": 2,
                        "handled_ids": ["aa"],
                        "unhandled_ids": ["bb"],
                        "transfer_limited_ids": ["cc"],
                        "rejected_ids": []
                    }
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .propagation_peer_sync(crate::PropagationPeerSyncRequest {
            peer: "peer-prop-a".to_string(),
            transfer_limit_kb: Some(42.5),
            wanted_ids: Some(json!(["aa"])),
            maintenance_claimed: false,
            force_sync: true,
        })
        .expect("propagation peer sync");

    assert_eq!(result.peer, "peer-prop-a");
    assert_eq!(result.peer_type.as_deref(), Some("manual"));
    assert!(!result.synced);
    assert!(result.postponed);
    assert_eq!(result.postpone_reason.as_deref(), Some("backoff"));
    assert_eq!(result.next_sync_attempt, Some(1_700_000_700));
    assert_eq!(result.messages["unhandled_ids"], json!(["bb"]));
    assert_eq!(result.propagation["transfer_limited_ids"], json!(["cc"]));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    assert_eq!(
        request.params,
        Some(json!({
            "operation_id": "app.propagation.peer_sync",
            "kind": "command",
            "target": null,
            "correlation_id": null,
            "timeout_ms": null,
            "payload": {
                "peer": "peer-prop-a",
                "transfer_limit_kb": 42.5,
                "wanted_ids": ["aa"],
                "force_sync": true
            },
            "extensions": {}
        }))
    );
    server.join().expect("server joined");
}
