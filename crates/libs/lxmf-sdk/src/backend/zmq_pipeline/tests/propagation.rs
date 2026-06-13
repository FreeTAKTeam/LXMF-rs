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

#[test]
fn propagation_remote_lifecycle_uses_zmq_sdk_envelopes_and_preserves_raw_state() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "response": {
                    "operation_id": "app.propagation.remote_status",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "remote": "remote-a",
                        "status": {
                            "state": "online",
                            "queue_depth": 3
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.remote_fetch",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "remote": "remote-a",
                        "propagation": {
                            "state_name": "completed",
                            "sync_progress": 1.0
                        },
                        "result": {
                            "synced": true,
                            "imported_count": 2,
                            "imported_ids": ["id-a", "id-b"],
                            "transferred_bytes": 128
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.remote_download",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "remote": "remote-a",
                        "propagation": {
                            "state_name": "failed",
                            "last_sync_error": "remote download postponed"
                        },
                        "result": {
                            "synced": false,
                            "postponed": true,
                            "postpone_reason": "timeout"
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.remote_sync",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "remote": "remote-a",
                        "peer": "peer-a",
                        "propagation": {
                            "state_name": "completed"
                        },
                        "peer_sync": {
                            "peer": "peer-a",
                            "synced": true,
                            "messages": {
                                "unhandled_ids": ["retry-a"]
                            }
                        },
                        "result": {
                            "synced": true
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.remote_unpeer",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "remote": "remote-a",
                        "peer": "peer-a",
                        "removed": true,
                        "propagation_cleared": 1,
                        "propagation_cleared_bytes": 64,
                        "messages": {
                            "offered": 0,
                            "unhandled_ids": []
                        },
                        "result": {
                            "accepted": true
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

    let status = client
        .propagation_remote_status(crate::PropagationRemoteRequest {
            remote: "remote-a".to_string(),
            identity_private_key_hex: Some("feedface".to_string()),
            timeout_secs: Some(2.5),
            transfer_limit_kb: None,
        })
        .expect("remote status");
    let fetch = client
        .propagation_remote_fetch(crate::PropagationRemoteRequest {
            remote: "remote-a".to_string(),
            identity_private_key_hex: None,
            timeout_secs: Some(8.0),
            transfer_limit_kb: Some(42.5),
        })
        .expect("remote fetch");
    let download = client
        .propagation_remote_download(crate::PropagationRemoteRequest {
            remote: "remote-a".to_string(),
            identity_private_key_hex: None,
            timeout_secs: Some(5.0),
            transfer_limit_kb: Some(84.0),
        })
        .expect("remote download");
    let sync = client
        .propagation_remote_sync(crate::PropagationRemotePeerRequest {
            remote: "remote-a".to_string(),
            peer: "peer-a".to_string(),
            identity_private_key_hex: None,
            timeout_secs: Some(5.0),
            transfer_limit_kb: Some(42.5),
        })
        .expect("remote sync");
    let unpeer = client
        .propagation_remote_unpeer(crate::PropagationRemotePeerRequest {
            remote: "remote-a".to_string(),
            peer: "peer-a".to_string(),
            identity_private_key_hex: None,
            timeout_secs: Some(5.0),
            transfer_limit_kb: None,
        })
        .expect("remote unpeer");

    assert_eq!(status.remote, "remote-a");
    assert_eq!(status.status["queue_depth"], json!(3));
    assert_eq!(fetch.result["imported_ids"], json!(["id-a", "id-b"]));
    assert_eq!(download.result["postpone_reason"], json!("timeout"));
    assert_eq!(download.propagation["last_sync_error"], json!("remote download postponed"));
    assert_eq!(sync.peer.as_deref(), Some("peer-a"));
    assert_eq!(sync.peer_sync["messages"]["unhandled_ids"], json!(["retry-a"]));
    assert!(unpeer.removed);
    assert_eq!(unpeer.propagation_cleared, Some(1));
    assert_eq!(unpeer.messages["unhandled_ids"], json!([]));

    let captured = captured.lock().expect("captured requests");
    let methods = captured.iter().map(|request| request.method.as_str()).collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec![
            "sdk_envelope_execute_v2",
            "sdk_envelope_execute_v2",
            "sdk_envelope_execute_v2",
            "sdk_envelope_execute_v2",
            "sdk_envelope_execute_v2",
        ]
    );
    let operation_ids = captured
        .iter()
        .map(|request| {
            request
                .params
                .as_ref()
                .expect("params")
                .get("operation_id")
                .cloned()
                .expect("operation id")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        operation_ids,
        vec![
            json!("app.propagation.remote_status"),
            json!("app.propagation.remote_fetch"),
            json!("app.propagation.remote_download"),
            json!("app.propagation.remote_sync"),
            json!("app.propagation.remote_unpeer"),
        ]
    );
    let kinds = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params").get("kind").cloned().expect("kind"))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            json!("query"),
            json!("command"),
            json!("command"),
            json!("command"),
            json!("command")
        ]
    );
    assert_eq!(
        captured[0].params.as_ref().expect("params")["payload"],
        json!({
            "remote": "remote-a",
            "identity_private_key_hex": "feedface",
            "timeout_secs": 2.5
        })
    );
    assert_eq!(
        captured[3].params.as_ref().expect("params")["payload"],
        json!({
            "remote": "remote-a",
            "peer": "peer-a",
            "timeout_secs": 5.0,
            "transfer_limit_kb": 42.5
        })
    );
    server.join().expect("server joined");
}

#[test]
fn propagation_sync_acknowledge_uses_zmq_sdk_envelope_and_preserves_state() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.propagation.acknowledge_sync_completion",
                "kind": "result",
                "accepted": true,
                "correlation_id": null,
                "payload": {
                    "propagation": {
                        "sync_state": 254,
                        "state_name": "failed",
                        "sync_progress": 0.0,
                        "last_sync_error": "remote sync timed out",
                        "retry_count": 3,
                        "queue_depth": 2
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
        .propagation_acknowledge_sync_completion(crate::PropagationAcknowledgeSyncRequest {
            reset_state: true,
            failure_state: Some(0xfe),
        })
        .expect("acknowledge propagation sync");

    assert_eq!(result.propagation["sync_state"], json!(254));
    assert_eq!(result.propagation["state_name"], json!("failed"));
    assert_eq!(result.propagation["last_sync_error"], json!("remote sync timed out"));
    assert_eq!(result.propagation["retry_count"], json!(3));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    assert_eq!(
        request.params,
        Some(json!({
            "operation_id": "app.propagation.acknowledge_sync_completion",
            "kind": "command",
            "target": null,
            "correlation_id": null,
            "timeout_ms": null,
            "payload": {
                "reset_state": true,
                "failure_state": 254
            },
            "extensions": {}
        }))
    );
    server.join().expect("server joined");
}

#[test]
fn propagation_node_lifecycle_uses_zmq_sdk_envelopes_and_preserves_router_state() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "response": {
                    "operation_id": "app.propagation.node.get",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "peer": null,
                        "meta": {
                            "queue_depth": 0
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.node.set",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "peer": "router-a",
                        "meta": {
                            "selected": true
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.node.list",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "nodes": [
                            {
                                "peer": "router-a",
                                "name": "Router A",
                                "last_seen": 1700000000,
                                "capabilities": ["propagation", "lxmf"],
                                "selected": true
                            }
                        ],
                        "meta": {
                            "node_count": 1
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

    let initial = client.propagation_node_get().expect("get propagation node");
    let selected = client
        .propagation_node_set(crate::PropagationNodeSetRequest {
            peer: Some("router-a".to_string()),
        })
        .expect("set propagation node");
    let listed = client.propagation_node_list().expect("list propagation nodes");

    assert_eq!(initial.peer, None);
    assert_eq!(initial.meta["queue_depth"], json!(0));
    assert_eq!(selected.peer.as_deref(), Some("router-a"));
    assert_eq!(selected.meta["selected"], json!(true));
    assert_eq!(listed.nodes[0]["peer"], json!("router-a"));
    assert_eq!(listed.nodes[0]["selected"], json!(true));
    assert_eq!(listed.meta["node_count"], json!(1));

    let captured = captured.lock().expect("captured requests");
    let operation_ids = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params")["operation_id"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        operation_ids,
        vec![
            json!("app.propagation.node.get"),
            json!("app.propagation.node.set"),
            json!("app.propagation.node.list"),
        ]
    );
    let kinds = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params")["kind"].clone())
        .collect::<Vec<_>>();
    assert_eq!(kinds, vec![json!("query"), json!("command"), json!("query")]);
    assert_eq!(captured[0].params.as_ref().expect("params")["payload"], json!({}));
    assert_eq!(
        captured[1].params.as_ref().expect("params")["payload"],
        json!({ "peer": "router-a" })
    );
    assert_eq!(captured[2].params.as_ref().expect("params")["payload"], json!({}));
    server.join().expect("server joined");
}
