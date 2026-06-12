#[test]
fn propagation_enable_queues_existing_entries_for_static_peers() {
    let daemon = RpcDaemon::test_instance_with_identity(hex::encode([2u8; 16]));
    let peer = hex::encode([0x51_u8; 16]);
    let entry = PropagationEntryRecord {
        transient_id: "a7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_101,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    daemon
        .handle_rpc(rpc_request(
            26,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": [peer.as_str()],
            }),
        ))
        .expect("enable propagation");
    assert_eq!(make_ready_propagation_peer(&daemon, 0x51), peer);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 27, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let result = daemon
        .handle_rpc(rpc_request(
            28,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn propagation_ingest_queues_new_entries_for_static_peers() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            26,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-ingest-queue"],
            }),
        ))
        .expect("enable propagation");

    let payload_hex = format!("{}{}", "12".repeat(16), "34".repeat(24));
    let ingest = daemon
        .handle_rpc(rpc_request(
            27,
            "propagation_ingest",
            json!({
                "payload_hex": payload_hex,
            }),
        ))
        .expect("ingest propagation")
        .result
        .expect("ingest result");
    let transient_id = ingest["transient_id"].as_str().expect("transient id");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 28, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-ingest-queue"))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(transient_id)]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("peer-static-ingest-queue").expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[json!(transient_id)]
    );
}

#[test]
fn propagation_purge_removes_deleted_entries_from_peer_record_snapshots() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            26,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-purge-queue"],
            }),
        ))
        .expect("enable propagation");

    let destination = [0x42_u8; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" purge queued propagation");
    let transient_id = daemon
        .ingest_propagation_payload_bytes_at_cost(payload.as_slice(), None, 0)
        .expect("ingest propagation");

    {
        let peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get("peer-static-purge-queue").expect("stored peer");
        let serialized = serde_json::to_value(record).expect("serialize peer record");
        assert_eq!(
            serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
            &[json!(transient_id.as_str())]
        );
    }

    let transient_bytes = hex::decode(transient_id.as_str()).expect("transient id hex");
    let purged = daemon.purge_propagation_payloads_for_destination(
        &destination,
        &[transient_bytes],
    );
    assert!(purged > 0);

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation_ids("peer-static-purge-queue")
            .expect("live unhandled ids")
            .is_empty(),
        "live store queue should not retain the purged entry"
    );
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("peer-static-purge-queue").expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn propagation_ingest_does_not_reopen_handled_peer_record_snapshot() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            26,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-duplicate-handled"],
            }),
        ))
        .expect("enable propagation");

    let destination = [0x43_u8; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" duplicate handled propagation");
    let transient_id = hex::encode(Sha256::digest(payload.as_slice()));
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: hex::encode(destination),
            payload_hex: hex::encode(payload.as_slice()),
            received_at: 1_700_000_112,
            size_bytes: payload.len() as u64,
            stamp_value: None,
        })
        .expect("store handled propagation");
    daemon
        .store
        .mark_peer_handled_propagation("peer-static-duplicate-handled", transient_id.as_str())
        .expect("mark handled propagation");

    let duplicate = daemon
        .ingest_propagation_payload_bytes_at_cost(payload.as_slice(), None, 0)
        .expect("duplicate ingest");
    assert_eq!(duplicate, transient_id);
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation_ids("peer-static-duplicate-handled")
            .expect("live unhandled ids")
            .is_empty(),
        "duplicate ingest should not reopen a handled live queue mark"
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-static-duplicate-handled")
            .expect("live handled ids"),
        vec![transient_id.clone()]
    );

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("peer-static-duplicate-handled").expect("stored peer");
    let serialized = serde_json::to_value(record).expect("serialize peer record");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(transient_id.as_str())]
    );
    assert_eq!(
        serialized["unhandled_ids"].as_array().expect("serialized unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_propagation_ingest_matches_source_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            29,
            "peer_sync",
            json!({ "peer": "Peer-Case-Source" }),
        ))
        .expect("seed mixed-case peer");
    daemon
        .handle_rpc(rpc_request(30, "peer_sync", json!({ "peer": "peer-case-relay" })))
        .expect("seed relay peer");

    let payload = b"mixed-case-source-peer-payload";
    let transient_id = daemon
        .ingest_peer_propagation_payload_bytes_at_cost(
            payload,
            None,
            0,
            "peer-case-source",
        )
        .expect("peer propagation ingest");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("Peer-Case-Source")
            .expect("source unhandled")
            .is_empty(),
        "source peer should not be offered its own inbound payload"
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("Peer-Case-Source")
            .expect("source handled ids"),
        vec![transient_id.clone()]
    );
    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-case-relay")
        .expect("relay unhandled");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

    let status = daemon
        .handle_rpc(RpcRequest { id: 31, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["unpeered_propagation_incoming"].as_u64(), Some(0));
    let peers = daemon
        .handle_rpc(RpcRequest { id: 32, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("Peer-Case-Source"))
        .expect("source peer row");
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));

    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get("Peer-Case-Source").expect("stored source peer");
    let serialized = serde_json::to_value(record).expect("serialize source peer");
    assert_eq!(
        serialized["handled_ids"].as_array().expect("serialized handled ids"),
        &[json!(transient_id)]
    );
}

#[test]
fn duplicate_peer_propagation_ingest_still_queues_relay_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    let source_peer = "peer-duplicate-source";
    let relay_peer = "peer-duplicate-relay";
    daemon
        .handle_rpc(rpc_request(29, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(30, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");

    let payload = b"known-source-peer-payload";
    let transient_id = hex::encode(Sha256::digest(payload));
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: hex::encode(&payload[..16]),
            payload_hex: hex::encode(payload),
            received_at: 1_700_000_113,
            size_bytes: payload.len() as u64,
            stamp_value: None,
        })
        .expect("seed known propagation entry");

    let duplicate = daemon
        .ingest_peer_propagation_payload_bytes_at_cost(payload, None, 0, source_peer)
        .expect("duplicate peer propagation ingest");
    assert_eq!(duplicate, transient_id);
    let repeated_duplicate = daemon
        .ingest_peer_propagation_payload_bytes_at_cost(payload, None, 0, source_peer)
        .expect("repeated duplicate peer propagation ingest");
    assert_eq!(repeated_duplicate, transient_id);

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids"),
        vec![transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(source_peer)
            .expect("source unhandled")
            .is_empty(),
        "source peer should not be re-offered its own duplicate payload"
    );

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer)
        .expect("relay unhandled");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 31, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer))
        .expect("source peer row");
    assert_eq!(source_row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
}

#[test]
fn peer_propagation_ingest_marks_inactive_source_received_for_later_activation_like_python() {
    let daemon = RpcDaemon::test_instance();
    let source_peer = "peer-late-inbound-source";
    let relay_peer = "peer-late-inbound-relay";
    daemon
        .handle_rpc(rpc_request(29, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");

    let payload = b"inactive-source-peer-payload";
    let transient_id = daemon
        .ingest_peer_propagation_payload_bytes_at_cost(payload, None, 0, source_peer)
        .expect("inactive source peer propagation ingest");

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("inactive source handled ids"),
        vec![transient_id.clone()],
        "inactive source should be marked received before later peer activation"
    );
    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer)
        .expect("relay unhandled");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

    let sync = daemon
        .handle_rpc(rpc_request(30, "peer_sync", json!({ "peer": source_peer })))
        .expect("activate source peer")
        .result
        .expect("peer sync result");
    assert_eq!(sync["propagation"]["transferred"].as_u64(), Some(0));
    assert!(
        sync["propagation"]["messages"].as_array().expect("transferred messages").is_empty()
    );
    assert_eq!(sync["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(
        sync["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(transient_id.as_str())]
    );
}

#[test]
fn accepted_peer_propagation_relay_rejects_ignored_destination_before_queueing() {
    let daemon = RpcDaemon::test_instance();
    let source_peer = "peer-ignored-source";
    let relay_peer = "peer-ignored-relay";
    let destination = [0x9a_u8; 16];
    let destination_hex = hex::encode(destination);
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" ignored accepted peer payload");
    let transient_id = hex::encode(Sha256::digest(&payload));

    daemon
        .handle_rpc(rpc_request(29, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(30, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    daemon
        .handle_rpc(rpc_request(
            31,
            "set_delivery_policy",
            json!({
                "ignored_destinations": [destination_hex],
            }),
        ))
        .expect("set ignored destination policy");

    let err = daemon
        .relay_accepted_peer_propagation_payload_bytes_at_cost(
            payload.as_slice(),
            Some(transient_id.as_str()),
            0,
            source_peer,
        )
        .expect_err("ignored accepted peer payload must be rejected");
    assert!(err.to_string().contains("ignored propagation destination"));
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

    assert!(
        daemon
            .store
            .get_propagation_entry(transient_id.as_str())
            .expect("load propagation entry")
            .is_none(),
        "ignored accepted peer payload must not be stored"
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(relay_peer)
            .expect("relay unhandled")
            .is_empty(),
        "ignored accepted peer payload must not be queued to relay peers"
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids")
            .is_empty(),
        "ignored accepted peer payload must not mark the source handled"
    );
}
