#[test]
fn propagation_remote_sync_marks_source_handled_and_queues_other_peers() {
    let payload = b"remote-sync-distribution-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = hex::encode([3u8; 16]);
    let relay_peer = hex::encode([4u8; 16]);
    let daemon =
        RpcDaemon::with_store(MessagesStore::in_memory().expect("store"), hex::encode([2u8; 16]));
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));
    daemon
        .handle_rpc(rpc_request(74, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    let pending_payload = b"remote-sync-preexisting-relay-pending";
    let pending_transient_id = hex::encode(Sha256::digest(pending_payload));
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: pending_transient_id.clone(),
            destination: "41".repeat(16),
            payload_hex: hex::encode(pending_payload),
            received_at: 1_700_000_745,
            size_bytes: pending_payload.len() as u64,
            stamp_value: None,
        })
        .expect("store preexisting relay pending payload");
    daemon
        .store
        .mark_peer_unhandled_propagation(relay_peer.as_str(), pending_transient_id.as_str())
        .expect("seed preexisting relay live queue mark");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(relay_peer.as_str()).expect("relay peer record");
        record.restored_unhandled_ids.clear();
        record.restored_handled_ids.clear();
    }

    let remote_sync = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": source_peer,
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(
        remote_sync["peer_sync"]["messages"]["handled_ids"]
            .as_array()
            .expect("source handled ids"),
        &[json!(transient_id.as_str())]
    );
    assert!(
        remote_sync["peer_sync"]["messages"]["unhandled_ids"]
            .as_array()
            .expect("source unhandled ids")
            .is_empty()
    );

    let source_handled = daemon
        .store
        .list_peer_handled_propagation_ids(source_peer.as_str())
        .expect("source handled");
    assert_eq!(source_handled, vec![transient_id.clone()]);
    let source_unhandled = daemon
        .store
        .list_peer_unhandled_propagation(source_peer.as_str())
        .expect("source unhandled");
    assert!(source_unhandled.is_empty());
    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer.as_str()))
        .expect("source peer row");
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(source_row["alive"].as_bool(), Some(true));
    let relay_unhandled = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer.as_str())
        .expect("relay unhandled");
    assert_eq!(relay_unhandled.len(), 2);
    assert!(relay_unhandled
        .iter()
        .any(|entry| entry.transient_id == pending_transient_id));
    assert!(relay_unhandled.iter().any(|entry| entry.transient_id == transient_id));
    let peer_records = daemon.peers.lock().expect("peers mutex poisoned");
    let relay_record = peer_records
        .get(relay_peer.as_str())
        .expect("relay peer record after remote sync");
    let serialized = serde_json::to_value(relay_record).expect("serialize relay peer");
    let restored_unhandled = serialized["unhandled_ids"]
        .as_array()
        .expect("serialized relay unhandled ids");
    assert!(restored_unhandled.contains(&json!(pending_transient_id.as_str())));
    assert!(restored_unhandled.contains(&json!(transient_id.as_str())));
}

#[test]
fn propagation_remote_sync_counts_source_incoming_after_prior_transfer_like_python() {
    let payload = b"remote-sync-prior-transfer-source-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = hex::encode([0x31_u8; 16]);
    let daemon =
        RpcDaemon::with_store(MessagesStore::in_memory().expect("store"), hex::encode([2u8; 16]));
    daemon
        .handle_rpc(rpc_request(76, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: "31".repeat(16),
            payload_hex: payload_hex.clone(),
            received_at: 1_700_000_731,
            size_bytes: payload.len() as u64,
            stamp_value: None,
        })
        .expect("seed known propagation entry");
    daemon
        .store
        .mark_peer_transferred_propagation(source_peer.as_str(), transient_id.as_str())
        .expect("mark prior transfer to source");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let remote_sync = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": source_peer,
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");

    assert_eq!(remote_sync["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(remote_sync["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(remote_sync["peer_sync"]["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(remote_sync["peer_sync"]["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(remote_sync["peer_sync"]["incoming"].as_u64(), Some(1));
}

#[test]
fn propagation_remote_sync_creates_missing_peer_record() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));

    daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-created",
            }),
        ))
        .expect("remote sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-sync-created"))
        .expect("peer row");
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert!(row["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
}

#[test]
fn propagation_remote_sync_imports_payloads_into_local_store() {
    let payload = b"remote-sync-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "imported_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(result["result"]["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after remote sync")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn propagation_remote_sync_imports_nested_peer_sync_messages_like_python() {
    let payload = b"remote-sync-nested-peer-sync-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": {
                "offered": 1,
                "outgoing": 1,
                "incoming": 0,
                "unhandled": 0,
                "handled_ids": [transient_id],
                "unhandled_ids": [],
            },
            "propagation": {
                "synced": true,
                "transferred": 1,
                "messages": [{
                    "transient_id": transient_id,
                    "payload_hex": payload_hex,
                }],
            },
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-nested-sync",
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(
        result["peer_sync"]["propagation"]["imported_count"].as_u64(),
        Some(1)
    );

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after nested remote sync")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn propagation_remote_sync_imports_binary_peer_sync_payloads_from_msgpack() {
    let payload = b"remote-sync-binary-peer-sync-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": {
                "offered": 1,
                "outgoing": 1,
                "incoming": 0,
                "unhandled": 0,
                "handled_ids": [transient_id],
                "unhandled_ids": [],
            },
            "propagation": {
                "synced": true,
                "transferred": 1,
                "messages": [{
                    "transient_id": transient_id,
                    "payload": payload.to_vec(),
                }],
            },
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-binary-sync",
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([transient_id]));
    assert_eq!(result["result"]["transferred_bytes"].as_u64(), Some(payload.len() as u64));

    daemon.propagation_payloads.lock().expect("propagation payload mutex poisoned").clear();
    let fetched = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after binary remote sync")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}
