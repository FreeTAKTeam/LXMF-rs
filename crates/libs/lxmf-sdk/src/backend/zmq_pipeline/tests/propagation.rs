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

#[test]
fn propagation_local_lifecycle_uses_zmq_sdk_envelopes_and_preserves_policy_state() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = spawn_response_sequence_zmq_server(
        command_endpoint.clone(),
        vec![
            json!({
                "response": {
                    "operation_id": "app.propagation.status",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "propagation": {
                            "enabled": false,
                            "sync_state": 0,
                            "state_name": "idle",
                            "selected_node": null
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.enable",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "propagation": {
                            "enabled": true,
                            "auth_required": true,
                            "static_peers": ["router-a"],
                            "sync_limit": 64
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.delivery_policy.get",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "policy": {
                            "auth_required": true,
                            "allowed_destinations": ["dest-allow"],
                            "denied_destinations": ["dest-deny"],
                            "ignored_destinations": [],
                            "prioritised_destinations": ["dest-priority"]
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.delivery_policy.set",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "policy": {
                            "auth_required": false,
                            "allowed_destinations": ["dest-allow"],
                            "denied_destinations": ["dest-deny-b"],
                            "ignored_destinations": ["dest-ignore"],
                            "prioritised_destinations": ["dest-priority"]
                        }
                    }
                }
            }),
            json!({
                "response": {
                    "operation_id": "app.propagation.peer_maintenance",
                    "kind": "result",
                    "accepted": true,
                    "correlation_id": null,
                    "payload": {
                        "timestamp": 1_700_001_000,
                        "culled": 1,
                        "culled_peers": ["peer-stale"],
                        "rotated": 1,
                        "rotated_peers": ["peer-slow"],
                        "synced_peer": "peer-sync",
                        "peer_sync": {
                            "peer": "peer-sync",
                            "postponed": false,
                            "messages": {
                                "unhandled_ids": ["msg-a"]
                            }
                        },
                        "max_unreachable_secs": 604800
                    }
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let status = client.propagation_status().expect("propagation status");
    let enabled = client
        .propagation_enable(crate::PropagationEnableRequest {
            enabled: true,
            auth_required: Some(true),
            store_root: Some("propagation-store".to_string()),
            target_cost: Some(12),
            stamp_cost_flexibility: Some(4),
            message_storage_limit_mb: Some(256),
            delivery_limit: Some(16),
            propagation_limit: Some(32),
            sync_limit: Some(64),
            autopeer: Some(true),
            autopeer_maxdepth: Some(2),
            static_peers: Some(vec!["router-a".to_string()]),
            max_peers: Some(8),
            from_static_only: Some(true),
            retain_synced_on_node: Some(false),
            peering_cost: Some(10),
            remote_peering_cost_max: Some(20),
        })
        .expect("propagation enable");
    let policy = client.propagation_delivery_policy_get().expect("delivery policy get");
    let updated_policy = client
        .propagation_delivery_policy_set(crate::PropagationDeliveryPolicyRequest {
            auth_required: Some(false),
            allowed_destinations: None,
            denied_destinations: Some(vec!["dest-deny-b".to_string()]),
            ignored_destinations: Some(vec!["dest-ignore".to_string()]),
            prioritised_destinations: None,
        })
        .expect("delivery policy set");
    let maintenance = client.propagation_peer_maintenance().expect("peer maintenance");

    assert_eq!(status.propagation["enabled"], json!(false));
    assert_eq!(enabled.propagation["static_peers"], json!(["router-a"]));
    assert_eq!(policy.policy["denied_destinations"], json!(["dest-deny"]));
    assert_eq!(updated_policy.policy["ignored_destinations"], json!(["dest-ignore"]));
    assert_eq!(maintenance.culled, 1);
    assert_eq!(maintenance.rotated_peers, vec!["peer-slow".to_string()]);
    assert_eq!(maintenance.peer_sync["messages"]["unhandled_ids"], json!(["msg-a"]));

    let captured = captured.lock().expect("captured requests");
    let operation_ids = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params")["operation_id"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        operation_ids,
        vec![
            json!("app.propagation.status"),
            json!("app.propagation.enable"),
            json!("app.propagation.delivery_policy.get"),
            json!("app.propagation.delivery_policy.set"),
            json!("app.propagation.peer_maintenance"),
        ]
    );
    let kinds = captured
        .iter()
        .map(|request| request.params.as_ref().expect("params")["kind"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![json!("query"), json!("command"), json!("query"), json!("command"), json!("command")]
    );
    assert_eq!(captured[0].params.as_ref().expect("params")["payload"], json!({}));
    assert_eq!(
        captured[1].params.as_ref().expect("params")["payload"],
        json!({
            "enabled": true,
            "auth_required": true,
            "store_root": "propagation-store",
            "target_cost": 12,
            "stamp_cost_flexibility": 4,
            "message_storage_limit_mb": 256,
            "delivery_limit": 16,
            "propagation_limit": 32,
            "sync_limit": 64,
            "autopeer": true,
            "autopeer_maxdepth": 2,
            "static_peers": ["router-a"],
            "max_peers": 8,
            "from_static_only": true,
            "retain_synced_on_node": false,
            "peering_cost": 10,
            "remote_peering_cost_max": 20
        })
    );
    assert_eq!(captured[2].params.as_ref().expect("params")["payload"], json!({}));
    assert_eq!(
        captured[3].params.as_ref().expect("params")["payload"],
        json!({
            "auth_required": false,
            "denied_destinations": ["dest-deny-b"],
            "ignored_destinations": ["dest-ignore"]
        })
    );
    assert_eq!(captured[4].params.as_ref().expect("params")["payload"], json!({}));
    server.join().expect("server joined");
}

#[test]
fn propagation_recovery_state_projects_status_for_zmq_sdk_clients() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "response": {
                "operation_id": "app.propagation.status",
                "kind": "result",
                "accepted": true,
                "correlation_id": null,
                "payload": {
                    "propagation": {
                        "enabled": true,
                        "selected_node": "router-recovery",
                        "sync_state": 254,
                        "state_name": "failed",
                        "sync_progress": 0.25,
                        "last_sync_started": 1_700_010_000,
                        "last_sync_completed": null,
                        "last_sync_error": "remote sync timed out",
                        "total_ingested": 7,
                        "last_ingest_count": 2,
                        "client_propagation_messages_received": 5,
                        "client_propagation_messages_served": 3
                    }
                }
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let state = client.propagation_recovery_state().expect("propagation recovery state");

    assert!(state.enabled);
    assert_eq!(state.selected_node.as_deref(), Some("router-recovery"));
    assert_eq!(state.sync_state, 254);
    assert_eq!(state.state_name.as_deref(), Some("failed"));
    assert_eq!(state.sync_progress, Some(0.25));
    assert_eq!(state.last_sync_started, Some(1_700_010_000));
    assert_eq!(state.last_sync_completed, None);
    assert_eq!(state.last_sync_error.as_deref(), Some("remote sync timed out"));
    assert_eq!(state.total_ingested, 7);
    assert_eq!(state.last_ingest_count, 2);
    assert_eq!(state.client_propagation_messages_received, 5);
    assert_eq!(state.client_propagation_messages_served, 3);
    assert_eq!(state.propagation["sync_state"], json!(254));

    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_envelope_execute_v2");
    assert_eq!(
        request.params,
        Some(json!({
            "operation_id": "app.propagation.status",
            "kind": "query",
            "target": null,
            "correlation_id": null,
            "timeout_ms": null,
            "payload": {},
            "extensions": {}
        }))
    );
    server.join().expect("server joined");
}

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
                        "transient_id": "transient-sdk-ingest"
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
                        "transferred_bytes": 18
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
    assert_eq!(fetched.transient_id, "transient-sdk-ingest");
    assert_eq!(fetched.payload_hex, "70726f7061676174696f6e2d7061796c6f6164");
    assert_eq!(fetched.transferred_bytes, 18);

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
