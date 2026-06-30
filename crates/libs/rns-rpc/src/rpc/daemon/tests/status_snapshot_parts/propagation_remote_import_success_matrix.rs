fn run_propagation_remote_import_success_side_effect_case(method: &str, case_tag: &str) {
    let payload = format!("{case_tag}-payload").into_bytes();
    let payload_hex = hex::encode(payload.as_slice());
    let transient_id = hex::encode(Sha256::digest(payload.as_slice()));
    let source_peer = format!("{case_tag}-source");
    let relay_peer = format!("{case_tag}-relay");
    let remote = if method == "propagation_remote_sync" {
        format!("{case_tag}-remote")
    } else {
        source_peer.clone()
    };
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(82, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(83, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(source_peer.as_str()).expect("source peer record");
        peer.alive = false;
        peer.last_sync_attempt = 111;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = if method == "propagation_remote_sync" {
            0
        } else {
            now_i64().saturating_add(12 * 60)
        };
    }
    let messages = json!([
        { "transient_id": transient_id, "payload_hex": payload_hex },
        { "transient_id": transient_id, "payload_hex": payload_hex },
    ]);
    let bridge_result = match method {
        "propagation_remote_sync" => json!({ "synced": true, "messages": messages }),
        "propagation_remote_fetch" => {
            json!({ "available_count": 2, "fetched_count": 2, "messages": messages })
        }
        "propagation_remote_download" => json!({ "downloaded_count": 2, "messages": messages }),
        _ => unreachable!("remote import matrix method: {method}"),
    };
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(bridge_result),
    }));

    let params = if method == "propagation_remote_sync" {
        json!({ "remote": remote, "peer": source_peer })
    } else {
        json!({ "remote": remote })
    };
    let response = daemon
        .handle_rpc(rpc_request(84, method, params))
        .expect("remote propagation success")
        .result
        .expect("remote propagation result");
    let result = &response["result"];
    assert_eq!(result["imported_count"].as_u64(), Some(1), "{method}");
    assert_eq!(result["duplicate_count"].as_u64(), Some(1), "{method}");
    assert_eq!(result["imported_ids"], json!([transient_id.as_str()]), "{method}");
    assert_eq!(result["transferred_bytes"].as_u64(), Some(payload.len() as u64), "{method}");
    assert_eq!(response["propagation"]["sync_state"].as_u64(), Some(0x07), "{method}");
    assert_eq!(response["propagation"]["state_name"].as_str(), Some("completed"), "{method}");
    assert_eq!(response["propagation"]["last_sync_error"], JsonValue::Null, "{method}");

    let duplicate_events: Vec<_> = std::iter::from_fn(|| daemon.take_event())
        .filter(|event| {
            event.event_type == "inbound_dropped" && event.payload["reason"] == json!("duplicate")
        })
        .collect();
    assert_eq!(duplicate_events.len(), 1, "{method} duplicate drop event count");
    let duplicate_event = &duplicate_events[0];
    assert_eq!(duplicate_event.payload["delivery_kind"], json!("propagation"), "{method}");
    assert_eq!(duplicate_event.payload["payload_mode"], json!("full_wire"), "{method}");
    assert_eq!(duplicate_event.payload["bytes_len"], json!(payload.len()), "{method}");
    assert_eq!(duplicate_event.payload["operation"], json!(method), "{method}");
    assert_eq!(duplicate_event.payload["transient_id"], json!(transient_id), "{method}");
    assert_eq!(
        duplicate_event.payload["detail"],
        json!("duplicate propagation payload in remote response"),
        "{method}"
    );
    if method == "propagation_remote_sync" {
        assert_eq!(duplicate_event.payload["peer"], json!(source_peer), "{method}");
    } else {
        assert!(duplicate_event.payload.get("peer").is_none(), "{method}");
    }

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(source_peer.as_str())
            .expect("source unhandled")
            .is_empty(),
        "{method} source should not retain unhandled work"
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer.as_str())
            .expect("source handled ids"),
        vec![transient_id.clone()],
        "{method}"
    );
    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation(relay_peer.as_str())
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1, "{method}");
    assert_eq!(relay_pending[0].transient_id, transient_id, "{method}");
    let peers = daemon
        .handle_rpc(RpcRequest { id: 85, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer.as_str()))
        .expect("source peer row");
    assert_eq!(source_row["incoming"].as_u64(), Some(1), "{method}");
    assert_eq!(source_row["messages"]["incoming"].as_u64(), Some(1), "{method}");
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64), "{method}");
    assert_eq!(source_row["alive"].as_bool(), Some(true), "{method}");
    assert_eq!(source_row["sync_backoff"].as_u64(), Some(0), "{method}");
    assert_eq!(source_row["next_sync_attempt"].as_i64(), Some(0), "{method}");
    assert!(
        source_row["last_sync_attempt"].as_i64().is_some_and(|value| value > 111),
        "{method} should refresh stale source backoff"
    );

    let peer_records = daemon.peers.lock().expect("peers mutex poisoned");
    let relay_record = peer_records.get(relay_peer.as_str()).expect("relay record");
    let relay_snapshot = serde_json::to_value(relay_record).expect("serialize relay");
    assert_eq!(
        relay_snapshot["unhandled_ids"]
            .as_array()
            .expect("relay snapshot unhandled ids"),
        &[json!(transient_id.as_str())],
        "{method}"
    );
}

#[test]
fn propagation_remote_import_success_matrix_covers_sync_fetch_and_download_side_effects() {
    run_propagation_remote_import_success_side_effect_case(
        "propagation_remote_sync",
        "remote-import-matrix-sync",
    );
    run_propagation_remote_import_success_side_effect_case(
        "propagation_remote_fetch",
        "remote-import-matrix-fetch",
    );
    run_propagation_remote_import_success_side_effect_case(
        "propagation_remote_download",
        "remote-import-matrix-download",
    );
}
