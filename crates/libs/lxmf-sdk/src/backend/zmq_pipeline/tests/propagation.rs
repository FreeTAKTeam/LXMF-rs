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
                    "postpone_reason": "timeout",
                    "failure_kind": "timeout",
                    "access_denied": false,
                    "last_sync_attempt": 1_700_000_100,
                    "next_sync_attempt": 1_700_000_700,
                    "sync_backoff": 600,
                    "messages": {
                        "offered": 2,
                        "outgoing": 1,
                        "incoming": 0,
                        "unhandled": 1,
                        "handled_ids": ["aa"],
                        "unhandled_ids": ["bb"]
                    },
                    "propagation": {
                        "transfer_limit": 42500,
                        "sync_limit": 84000,
                        "target_stamp_cost": 8,
                        "stamp_cost_flexibility": 2,
                        "offered": 2,
                        "transferred": 1,
                        "skipped": 1,
                        "rejected": 0,
                        "transfer_limited": 1,
                        "bytes": 512,
                        "remaining_bytes": 128,
                        "rejected_bytes": 0,
                        "transfer_limited_bytes": 64,
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
    assert_eq!(result.postpone_reason.as_deref(), Some("timeout"));
    assert_eq!(result.failure_kind.as_deref(), Some("timeout"));
    assert!(result.timed_out);
    assert!(!result.access_denied);
    assert_eq!(result.next_sync_attempt, Some(1_700_000_700));
    assert_eq!(result.transfer_limit, Some(42_500));
    assert_eq!(result.sync_limit, Some(84_000));
    assert_eq!(result.target_stamp_cost, Some(8));
    assert_eq!(result.stamp_cost_flexibility, Some(2));
    assert_eq!(result.queue.offered, 2);
    assert_eq!(result.queue.outgoing, 1);
    assert_eq!(result.queue.unhandled, 1);
    assert_eq!(result.queue.transferred, 1);
    assert_eq!(result.queue.skipped, 1);
    assert_eq!(result.queue.rejected, 0);
    assert_eq!(result.queue.transfer_limited, 1);
    assert_eq!(result.queue.transferred_bytes, 512);
    assert_eq!(result.queue.skipped_bytes, 128);
    assert_eq!(result.queue.rejected_bytes, 0);
    assert_eq!(result.queue.transfer_limited_bytes, 64);
    assert_eq!(result.queue.handled_ids, vec!["aa".to_string()]);
    assert_eq!(result.queue.unhandled_ids, vec!["bb".to_string()]);
    assert_eq!(result.queue.transfer_limited_ids, vec!["cc".to_string()]);
    assert_eq!(result.queue.rejected_ids, Vec::<String>::new());
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
                            "queue_depth": 3,
                            "selected_node": "node-a",
                            "selected_peer": "peer-a",
                            "failure_kind": "no_access",
                            "access_denied": true,
                            "retry_count": 2,
                            "next_sync_attempt": 1_700_000_900,
                            "last_sync_error": "previous timeout"
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
                            "selected_node": "node-fetch",
                            "selected_peer": "peer-fetch",
                            "sync_progress": 1.0,
                            "last_sync_started": 1_700_001_000,
                            "last_sync_completed": 1_700_001_120,
                            "transferred_ids": ["id-a"],
                            "skipped_ids": [],
                            "rejected_ids": [],
                            "transfer_limited_ids": [],
                            "failure_kind": null
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
                            "selected_node": "node-download",
                            "selected_peer": "peer-download",
                            "last_sync_error": "remote download postponed",
                            "last_sync_started": 1_700_001_800,
                            "last_sync_completed": null,
                            "failure_kind": "timeout",
                            "retry_count": 5,
                            "next_sync_attempt": 1_700_001_900,
                            "access_denied": false,
                            "transferred_ids": [],
                            "skipped_ids": ["id-c"],
                            "rejected_ids": [],
                            "transfer_limited_ids": ["id-d"]
                        },
                        "result": {
                            "synced": false,
                            "postponed": true,
                            "postpone_reason": "timeout",
                            "failure_kind": "timeout"
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
                            "state_name": "failed",
                            "selected_node": "node-sync",
                            "selected_peer": "peer-sync",
                            "last_sync_error": "remote sync timed out",
                            "last_sync_started": 1_700_002_000,
                            "last_sync_completed": null,
                            "failure_kind": "timeout",
                            "retry_count": 6,
                            "next_sync_attempt": 1_700_002_100,
                            "access_denied": false,
                            "transferred_ids": ["sync-done"],
                            "skipped_ids": ["sync-skipped"],
                            "rejected_ids": ["sync-rejected"],
                            "transfer_limited_ids": ["sync-limited"]
                        },
                        "peer_sync": {
                            "peer": "peer-a",
                            "synced": false,
                            "messages": {
                                "offered": 3,
                                "outgoing": 2,
                                "unhandled": 1,
                                "handled_ids": ["handled-a"],
                                "unhandled_ids": ["retry-a"]
                            },
                            "propagation": {
                                "postponed": true,
                                "postpone_reason": "backoff",
                                "transferred_ids": ["handled-a"],
                                "transfer_limited_ids": ["retry-a"]
                            }
                        },
                        "result": {
                            "synced": false,
                            "postponed": true,
                            "postpone_reason": "timeout",
                            "failure_kind": "timeout"
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
                        "removed": false,
                        "propagation_cleared": 1,
                        "propagation_cleared_bytes": 64,
                        "messages": {
                            "offered": 0,
                            "outgoing": 1,
                            "incoming": 0,
                            "unhandled": 1,
                            "offered_bytes": 0,
                            "unhandled_bytes": 64,
                            "handled_ids": ["done-a"],
                            "unhandled_ids": ["retry-cleaned"]
                        },
                        "propagation": {
                            "state_name": "failed",
                            "selected_node": "node-unpeer",
                            "selected_peer": "peer-unpeer",
                            "last_sync_error": "remote unpeer denied",
                            "last_sync_started": 1_700_002_600,
                            "last_sync_completed": null,
                            "retry_count": 7,
                            "next_sync_attempt": 1700002700,
                            "transferred_ids": ["done-a"],
                            "skipped_ids": ["retry-cleaned"],
                            "rejected_ids": ["denied-a"],
                            "transfer_limited_ids": []
                        },
                        "result": {
                            "accepted": false,
                            "synced": false,
                            "postponed": false,
                            "failure_kind": "no_access"
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
    assert_eq!(status.status_state.state.as_deref(), Some("online"));
    assert_eq!(status.status_state.queue_depth, 3);
    assert_eq!(status.status_state.selected_node.as_deref(), Some("node-a"));
    assert_eq!(status.status_state.selected_peer.as_deref(), Some("peer-a"));
    assert_eq!(status.status_state.failure_kind.as_deref(), Some("no_access"));
    assert!(!status.status_state.timed_out);
    assert!(status.status_state.access_denied);
    assert_eq!(status.status_state.retry_count, 2);
    assert_eq!(status.status_state.next_sync_attempt, Some(1_700_000_900));
    assert_eq!(status.status_state.last_sync_error.as_deref(), Some("previous timeout"));
    assert_eq!(fetch.result["imported_ids"], json!(["id-a", "id-b"]));
    assert!(fetch.transfer_state.synced);
    assert_eq!(fetch.transfer_state.imported_count, 2);
    assert_eq!(fetch.transfer_state.imported_ids, vec!["id-a".to_string(), "id-b".to_string()]);
    assert_eq!(fetch.transfer_state.transferred_bytes, 128);
    assert_eq!(fetch.transfer_state.state_name.as_deref(), Some("completed"));
    assert_eq!(fetch.transfer_state.selected_node.as_deref(), Some("node-fetch"));
    assert_eq!(fetch.transfer_state.selected_peer.as_deref(), Some("peer-fetch"));
    assert_eq!(fetch.transfer_state.sync_progress, Some(1.0));
    assert_eq!(fetch.transfer_state.last_sync_started, Some(1_700_001_000));
    assert_eq!(fetch.transfer_state.last_sync_completed, Some(1_700_001_120));
    assert_eq!(fetch.transfer_state.failure_kind, None);
    assert_eq!(fetch.queue.transferred_ids, vec!["id-a".to_string()]);
    assert_eq!(fetch.queue.skipped_ids, Vec::<String>::new());
    assert_eq!(fetch.queue.rejected_ids, Vec::<String>::new());
    assert_eq!(fetch.queue.transfer_limited_ids, Vec::<String>::new());
    assert_eq!(download.result["postpone_reason"], json!("timeout"));
    assert!(!download.transfer_state.synced);
    assert!(download.transfer_state.postponed);
    assert_eq!(download.transfer_state.postpone_reason.as_deref(), Some("timeout"));
    assert_eq!(download.transfer_state.state_name.as_deref(), Some("failed"));
    assert_eq!(download.transfer_state.selected_node.as_deref(), Some("node-download"));
    assert_eq!(download.transfer_state.selected_peer.as_deref(), Some("peer-download"));
    assert_eq!(download.transfer_state.failure_kind.as_deref(), Some("timeout"));
    assert!(download.transfer_state.timed_out);
    assert!(!download.transfer_state.access_denied);
    assert_eq!(download.transfer_state.retry_count, 5);
    assert_eq!(download.transfer_state.next_sync_attempt, Some(1_700_001_900));
    assert_eq!(download.transfer_state.last_sync_started, Some(1_700_001_800));
    assert_eq!(download.transfer_state.last_sync_completed, None);
    assert_eq!(
        download.transfer_state.last_sync_error.as_deref(),
        Some("remote download postponed")
    );
    assert_eq!(download.queue.transferred_ids, Vec::<String>::new());
    assert_eq!(download.queue.skipped_ids, vec!["id-c".to_string()]);
    assert_eq!(download.queue.rejected_ids, Vec::<String>::new());
    assert_eq!(download.queue.transfer_limited_ids, vec!["id-d".to_string()]);
    assert_eq!(download.propagation["last_sync_error"], json!("remote download postponed"));
    assert_eq!(sync.peer.as_deref(), Some("peer-a"));
    assert!(!sync.transfer_state.synced);
    assert!(sync.transfer_state.postponed);
    assert_eq!(sync.transfer_state.postpone_reason.as_deref(), Some("timeout"));
    assert_eq!(sync.transfer_state.state_name.as_deref(), Some("failed"));
    assert_eq!(sync.transfer_state.selected_node.as_deref(), Some("node-sync"));
    assert_eq!(sync.transfer_state.selected_peer.as_deref(), Some("peer-sync"));
    assert_eq!(sync.transfer_state.failure_kind.as_deref(), Some("timeout"));
    assert!(sync.transfer_state.timed_out);
    assert!(!sync.transfer_state.access_denied);
    assert_eq!(sync.transfer_state.retry_count, 6);
    assert_eq!(sync.transfer_state.next_sync_attempt, Some(1_700_002_100));
    assert_eq!(sync.transfer_state.last_sync_started, Some(1_700_002_000));
    assert_eq!(sync.transfer_state.last_sync_completed, None);
    assert_eq!(sync.transfer_state.last_sync_error.as_deref(), Some("remote sync timed out"));
    assert_eq!(sync.peer_sync["messages"]["unhandled_ids"], json!(["retry-a"]));
    let peer_sync_state = sync.peer_sync_state.as_ref().expect("typed peer sync state");
    assert_eq!(peer_sync_state.peer, "peer-a");
    assert!(!peer_sync_state.synced);
    assert!(peer_sync_state.postponed);
    assert_eq!(peer_sync_state.postpone_reason.as_deref(), Some("backoff"));
    assert_eq!(peer_sync_state.queue.offered, 3);
    assert_eq!(peer_sync_state.queue.handled_ids, vec!["handled-a".to_string()]);
    assert_eq!(peer_sync_state.queue.unhandled_ids, vec!["retry-a".to_string()]);
    assert_eq!(peer_sync_state.queue.transfer_limited_ids, vec!["retry-a".to_string()]);
    assert_eq!(sync.queue.transferred_ids, vec!["sync-done".to_string()]);
    assert_eq!(sync.queue.skipped_ids, vec!["sync-skipped".to_string()]);
    assert_eq!(sync.queue.rejected_ids, vec!["sync-rejected".to_string()]);
    assert_eq!(sync.queue.transfer_limited_ids, vec!["sync-limited".to_string()]);
    assert!(!unpeer.removed);
    assert_eq!(unpeer.propagation_cleared, Some(1));
    assert!(!unpeer.transfer_state.synced);
    assert!(!unpeer.transfer_state.postponed);
    assert_eq!(unpeer.transfer_state.state_name.as_deref(), Some("failed"));
    assert_eq!(unpeer.transfer_state.selected_node.as_deref(), Some("node-unpeer"));
    assert_eq!(unpeer.transfer_state.selected_peer.as_deref(), Some("peer-unpeer"));
    assert_eq!(unpeer.transfer_state.failure_kind.as_deref(), Some("no_access"));
    assert!(!unpeer.transfer_state.timed_out);
    assert!(unpeer.transfer_state.access_denied);
    assert_eq!(unpeer.transfer_state.retry_count, 7);
    assert_eq!(unpeer.transfer_state.next_sync_attempt, Some(1_700_002_700));
    assert_eq!(unpeer.transfer_state.last_sync_started, Some(1_700_002_600));
    assert_eq!(unpeer.transfer_state.last_sync_completed, None);
    assert_eq!(unpeer.transfer_state.last_sync_error.as_deref(), Some("remote unpeer denied"));
    assert_eq!(unpeer.messages["unhandled_ids"], json!(["retry-cleaned"]));
    assert_eq!(unpeer.queue.outgoing, 1);
    assert_eq!(unpeer.queue.unhandled, 1);
    assert_eq!(unpeer.queue.unhandled_bytes, 64);
    assert_eq!(unpeer.queue.handled_ids, vec!["done-a".to_string()]);
    assert_eq!(unpeer.queue.unhandled_ids, vec!["retry-cleaned".to_string()]);
    assert_eq!(unpeer.queue.transferred_ids, vec!["done-a".to_string()]);
    assert_eq!(unpeer.queue.skipped_ids, vec!["retry-cleaned".to_string()]);
    assert_eq!(unpeer.queue.rejected_ids, vec!["denied-a".to_string()]);

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
                        "failure_kind": "timeout",
                        "next_sync_attempt": 1_700_002_000,
                        "access_denied": false,
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
    assert_eq!(result.recovery_state.sync_state, 254);
    assert_eq!(result.recovery_state.state_name.as_deref(), Some("failed"));
    assert_eq!(result.recovery_state.last_sync_error.as_deref(), Some("remote sync timed out"));
    assert_eq!(result.recovery_state.failure_kind.as_deref(), Some("timeout"));
    assert!(result.recovery_state.timed_out);
    assert!(!result.recovery_state.access_denied);
    assert_eq!(result.recovery_state.next_sync_attempt, Some(1_700_002_000));
    assert_eq!(result.recovery_state.retry_count, 3);
    assert_eq!(result.recovery_state.queue_depth, 2);
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
                            "state_name": "idle",
                            "queue_depth": 0,
                            "retry_count": 0
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
                            "selected": false,
                            "state_name": "failed",
                            "failure_kind": "no_access",
                            "access_denied": true,
                            "queue_depth": 3,
                            "retry_count": 2,
                            "next_sync_attempt": 1700000600,
                            "last_sync_error": "router denied"
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
    assert_eq!(initial.selection_state.peer, None);
    assert_eq!(initial.selection_state.state.as_deref(), Some("idle"));
    assert!(!initial.selection_state.selected);
    assert_eq!(initial.selection_state.queue_depth, 0);
    assert_eq!(selected.peer.as_deref(), Some("router-a"));
    assert_eq!(selected.meta["selected"], json!(false));
    assert_eq!(selected.selection_state.peer.as_deref(), Some("router-a"));
    assert_eq!(selected.selection_state.state.as_deref(), Some("failed"));
    assert!(selected.selection_state.selected);
    assert_eq!(selected.selection_state.failure_kind.as_deref(), Some("no_access"));
    assert!(!selected.selection_state.timed_out);
    assert!(selected.selection_state.access_denied);
    assert_eq!(selected.selection_state.queue_depth, 3);
    assert_eq!(selected.selection_state.retry_count, 2);
    assert_eq!(selected.selection_state.next_sync_attempt, Some(1_700_000_600));
    assert_eq!(selected.selection_state.last_sync_error.as_deref(), Some("router denied"));
    assert_eq!(listed.nodes[0]["peer"], json!("router-a"));
    assert_eq!(listed.nodes[0]["selected"], json!(true));
    assert_eq!(listed.node_records.len(), 1);
    assert_eq!(listed.node_records[0].peer.as_deref(), Some("router-a"));
    assert_eq!(listed.node_records[0].name.as_deref(), Some("Router A"));
    assert_eq!(listed.node_records[0].last_seen, Some(1_700_000_000));
    assert!(listed.node_records[0].selected);
    assert_eq!(
        listed.node_records[0].capabilities,
        vec!["propagation".to_string(), "lxmf".to_string()]
    );
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
                            "selected_node": null,
                            "retry_count": 1,
                            "queue_depth": 2
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
                            "sync_state": 1,
                            "state_name": "syncing",
                            "queue_depth": 4,
                            "auth_required": true,
                            "store_root": "propagation-store",
                            "target_cost": 12,
                            "stamp_cost_flexibility": 4,
                            "message_storage_limit_mb": 256,
                            "delivery_limit": 16,
                            "propagation_limit": 32,
                            "autopeer": true,
                            "autopeer_maxdepth": 2,
                            "static_peers": ["router-a"],
                            "sync_limit": 64,
                            "max_peers": 8,
                            "from_static_only": true,
                            "retain_synced_on_node": false,
                            "peering_cost": 10,
                            "remote_peering_cost_max": 20
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
                            "synced": true,
                            "postponed": false,
                            "last_sync_attempt": 1_700_000_800,
                            "next_sync_attempt": 1_700_001_400,
                            "messages": {
                                "offered": 2,
                                "outgoing": 1,
                                "unhandled": 1,
                                "handled_ids": ["msg-handled"],
                                "unhandled_ids": ["msg-a"]
                            },
                            "propagation": {
                                "transferred_ids": ["msg-handled"],
                                "transfer_limited_ids": ["msg-a"]
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
    assert!(!status.recovery_state.enabled);
    assert_eq!(status.recovery_state.state_name.as_deref(), Some("idle"));
    assert_eq!(status.recovery_state.queue_depth, 2);
    assert_eq!(status.recovery_state.retry_count, 1);
    assert_eq!(enabled.propagation["static_peers"], json!(["router-a"]));
    assert!(enabled.recovery_state.enabled);
    assert_eq!(enabled.recovery_state.sync_state, 1);
    assert_eq!(enabled.recovery_state.state_name.as_deref(), Some("syncing"));
    assert_eq!(enabled.recovery_state.queue_depth, 4);
    assert!(enabled.recovery_state.auth_required);
    assert_eq!(enabled.recovery_state.store_root.as_deref(), Some("propagation-store"));
    assert_eq!(enabled.recovery_state.target_cost, Some(12));
    assert_eq!(enabled.recovery_state.stamp_cost_flexibility, Some(4));
    assert_eq!(enabled.recovery_state.message_storage_limit_mb, Some(256));
    assert_eq!(enabled.recovery_state.delivery_limit, Some(16));
    assert_eq!(enabled.recovery_state.propagation_limit, Some(32));
    assert_eq!(enabled.recovery_state.autopeer, Some(true));
    assert_eq!(enabled.recovery_state.autopeer_maxdepth, Some(2));
    assert_eq!(enabled.recovery_state.static_peers, vec!["router-a".to_string()]);
    assert_eq!(enabled.recovery_state.sync_limit, Some(64));
    assert_eq!(enabled.recovery_state.max_peers, Some(8));
    assert_eq!(enabled.recovery_state.from_static_only, Some(true));
    assert_eq!(enabled.recovery_state.retain_synced_on_node, Some(false));
    assert_eq!(enabled.recovery_state.peering_cost, Some(10));
    assert_eq!(enabled.recovery_state.remote_peering_cost_max, Some(20));
    assert_eq!(policy.policy["denied_destinations"], json!(["dest-deny"]));
    assert!(policy.policy_state.auth_required);
    assert_eq!(policy.policy_state.allowed_destinations, vec!["dest-allow".to_string()]);
    assert_eq!(policy.policy_state.denied_destinations, vec!["dest-deny".to_string()]);
    assert_eq!(policy.policy_state.ignored_destinations, Vec::<String>::new());
    assert_eq!(policy.policy_state.prioritised_destinations, vec!["dest-priority".to_string()]);
    assert_eq!(updated_policy.policy["ignored_destinations"], json!(["dest-ignore"]));
    assert!(!updated_policy.policy_state.auth_required);
    assert_eq!(updated_policy.policy_state.denied_destinations, vec!["dest-deny-b".to_string()]);
    assert_eq!(updated_policy.policy_state.ignored_destinations, vec!["dest-ignore".to_string()]);
    assert_eq!(maintenance.culled, 1);
    assert_eq!(maintenance.rotated_peers, vec!["peer-slow".to_string()]);
    assert_eq!(maintenance.peer_sync["messages"]["unhandled_ids"], json!(["msg-a"]));
    let maintenance_sync =
        maintenance.peer_sync_state.as_ref().expect("typed maintenance peer sync state");
    assert_eq!(maintenance_sync.peer, "peer-sync");
    assert!(maintenance_sync.synced);
    assert_eq!(maintenance_sync.last_sync_attempt, Some(1_700_000_800));
    assert_eq!(maintenance_sync.next_sync_attempt, Some(1_700_001_400));
    assert_eq!(maintenance_sync.queue.offered, 2);
    assert_eq!(maintenance_sync.queue.outgoing, 1);
    assert_eq!(maintenance_sync.queue.unhandled, 1);
    assert_eq!(maintenance_sync.queue.handled_ids, vec!["msg-handled".to_string()]);
    assert_eq!(maintenance_sync.queue.unhandled_ids, vec!["msg-a".to_string()]);
    assert_eq!(maintenance_sync.queue.transferred_ids, vec!["msg-handled".to_string()]);
    assert_eq!(maintenance_sync.queue.transfer_limited_ids, vec!["msg-a".to_string()]);

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
                        "failure_kind": "timeout",
                        "next_sync_attempt": 1_700_010_900,
                        "access_denied": false,
                        "retry_count": 4,
                        "queue_depth": 9,
                        "timestamp": 1_700_010_500,
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
    assert_eq!(state.failure_kind.as_deref(), Some("timeout"));
    assert!(state.timed_out);
    assert!(!state.access_denied);
    assert_eq!(state.next_sync_attempt, Some(1_700_010_900));
    assert_eq!(state.retry_count, 4);
    assert_eq!(state.queue_depth, 9);
    assert_eq!(state.timestamp, Some(1_700_010_500));
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
