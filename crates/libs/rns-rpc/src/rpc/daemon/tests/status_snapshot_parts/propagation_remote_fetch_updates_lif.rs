#[test]
fn propagation_remote_fetch_updates_lifecycle_status() {
    let payload = b"remote-fetch-lifecycle-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "imported_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            75,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect("remote fetch");

    let status = daemon
        .handle_rpc(RpcRequest { id: 76, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x07));
    assert_eq!(propagation["state_name"].as_str(), Some("completed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(1.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn propagation_remote_fetch_derives_missing_transient_id_from_payload_bytes() {
    let payload = b"remote-payload-without-id";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "imported_count": 1,
            "payloads": [{
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect("remote fetch");

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            75,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after remote import")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn propagation_remote_fetch_rejects_mismatched_transient_id() {
    let payload_hex = hex::encode(b"remote-payload-with-mismatched-id");
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "imported_count": 1,
            "messages": [{
                "transient_id": "aa".repeat(32),
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect_err("mismatched remote transient_id must be rejected");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("transient_id does not match propagation payload"));
    assert!(
        daemon
            .store
            .get_propagation_entry("aa".repeat(32).as_str())
            .expect("load bogus transient id")
            .is_none()
    );
}

#[test]
fn propagation_remote_fetch_rejects_mixed_batch_without_partial_import_side_effects() {
    let valid_payload = b"remote-fetch-valid-before-invalid";
    let valid_payload_hex = hex::encode(valid_payload);
    let valid_transient_id = hex::encode(Sha256::digest(valid_payload));
    let invalid_payload_hex = hex::encode(b"remote-fetch-invalid-after-valid");
    let relay_peer = "peer-fetch-atomic-relay";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(77, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 2,
            "fetched_count": 2,
            "imported_count": 2,
            "messages": [
                {
                    "transient_id": valid_transient_id,
                    "payload_hex": valid_payload_hex,
                },
                {
                    "transient_id": "aa".repeat(32),
                    "payload_hex": invalid_payload_hex,
                }
            ],
        })),
    }));

    let err = daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect_err("mixed remote import batch should reject atomically");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("transient_id does not match propagation payload"));
    assert!(
        daemon
            .store
            .get_propagation_entry(valid_transient_id.as_str())
            .expect("load valid transient id")
            .is_none(),
        "valid payload preceding an invalid payload must not be persisted"
    );
    assert!(
        !daemon
            .propagation_payloads
            .lock()
            .expect("propagation payload mutex poisoned")
            .contains_key(valid_transient_id.as_str()),
        "valid payload preceding an invalid payload must not be cached in memory"
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(relay_peer)
            .expect("relay pending")
            .is_empty(),
        "rejected mixed batch must not queue relay work"
    );
}

#[test]
fn failed_propagation_remote_fetch_import_updates_lifecycle_error() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "imported_count": 1,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));

    let err = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect_err("remote fetch import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 78, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert!(propagation["last_sync_error"]
        .as_str()
        .is_some_and(|value| value.contains("invalid remote propagation payload hex")));
}

#[test]
fn denied_access_propagation_remote_fetch_sets_no_access_lifecycle_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteAccessDeniedBridge));

    let err = daemon
        .handle_rpc(rpc_request(77, "propagation_remote_fetch", json!({ "remote": "remote-node" })))
        .expect_err("remote fetch access denial should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node denied access");

    let status = daemon
        .handle_rpc(RpcRequest { id: 78, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xf4));
    assert_eq!(propagation["state_name"].as_str(), Some("no_access"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("propagation node denied access"));
}

#[test]
fn propagation_remote_download_imports_payloads_into_local_store() {
    let payload = b"remote-download-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-download-relay" })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "imported_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_download",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect("remote download")
        .result
        .expect("remote download result");
    assert_eq!(result["propagation"]["sync_state"].as_u64(), Some(0x07));
    assert_eq!(result["propagation"]["state_name"].as_str(), Some("completed"));
    assert_eq!(result["propagation"]["sync_progress"].as_f64(), Some(1.0));
    assert!(result["propagation"]["last_sync_started"].as_i64().is_some());
    assert!(result["propagation"]["last_sync_completed"].as_i64().is_some());
    assert_eq!(result["propagation"]["last_sync_error"], JsonValue::Null);
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(result["result"]["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after remote download")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-download-relay")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);
}

#[test]
fn propagation_remote_download_marks_source_received_and_queues_other_peers() {
    let payload = b"remote-download-source-peer-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-download-source";
    let relay_peer = "peer-download-source-relay";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_download",
            json!({ "remote": source_peer }),
        ))
        .expect("remote download from source peer");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(source_peer)
            .expect("source unhandled")
            .is_empty(),
        "remote source should not be offered the payload it supplied"
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids"),
        vec![transient_id.clone()]
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 81, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer))
        .expect("source peer row");
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(source_row["alive"].as_bool(), Some(true));
    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer)
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);
}

#[test]
fn propagation_remote_download_marks_inactive_source_received_for_later_activation_like_python() {
    let payload = b"remote-download-inactive-source-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-download-late-source";
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_download",
            json!({ "remote": source_peer }),
        ))
        .expect("remote download from inactive source");

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("inactive source handled ids"),
        vec![transient_id.clone()],
        "inactive source should be marked received before later peer activation"
    );

    let sync = daemon
        .handle_rpc(rpc_request(83, "peer_sync", json!({ "peer": source_peer })))
        .expect("activate source peer")
        .result
        .expect("peer sync result");
    assert_eq!(sync["propagation"]["transferred"].as_u64(), Some(0));
    assert!(sync["propagation"]["messages"].as_array().expect("transferred messages").is_empty());
    assert_eq!(sync["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(
        sync["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(transient_id.as_str())]
    );
}
