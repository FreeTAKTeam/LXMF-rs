#[test]
fn selected_propagation_node_matches_existing_peer_case_insensitively_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let stored_peer = "Ef".repeat(16);
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .accept_announce_with_metadata(
            stored_peer.clone(),
            1_700_000_608,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(1),
            Some(Some(1)),
            Some(Some(1)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept mixed-case propagation peer");
    let entry = PropagationEntryRecord {
        transient_id: "ae".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "35".repeat(24),
        received_at: 1_700_000_609,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    let result = daemon
        .handle_rpc(rpc_request(
            75,
            "set_outbound_propagation_node",
            json!({ "peer": request_peer }),
        ))
        .expect("set propagation node")
        .result
        .expect("set propagation node result");
    assert_eq!(result["peer"].as_str(), Some(stored_peer.as_str()));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 76,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"].as_str(), Some(stored_peer.as_str()));

    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer.as_str())
            .expect("stored peer unhandled")
            .len(),
        1
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer.to_ascii_lowercase().as_str())
            .expect("lowercase peer unhandled")
            .len(),
        1
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let peer_rows = peers["peers"].as_array().expect("peer rows");
    let matching_rows = peer_rows
        .iter()
        .filter(|row| row["peer"].as_str().is_some_and(|peer| peer.eq_ignore_ascii_case(stored_peer.as_str())))
        .collect::<Vec<_>>();
    assert_eq!(matching_rows.len(), 1);
    assert_eq!(matching_rows[0]["peer"].as_str(), Some(stored_peer.as_str()));
    assert_eq!(matching_rows[0]["messages"]["unhandled"].as_u64(), Some(1));
}

#[test]
fn outbound_propagation_cost_reads_selected_node_app_data_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            91,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let peer = "aabbccddeeff00112233445566778899";
    let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::from(1_700_000_701i64),
        rmpv::Value::Boolean(true),
        rmpv::Value::from(256),
        rmpv::Value::from(2048),
        rmpv::Value::Array(vec![rmpv::Value::from(27), rmpv::Value::from(5), rmpv::Value::from(11)]),
        rmpv::Value::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");
    daemon
        .handle_rpc(rpc_request(
            92,
            "announce_received",
            json!({
                "peer": peer,
                "timestamp": 1_700_000_701i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("propagation announce");
    daemon
        .handle_rpc(rpc_request(93, "set_outbound_propagation_node", json!({ "peer": peer })))
        .expect("select propagation node");

    let result = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_cost".to_string(),
            params: None,
        })
        .expect("get outbound propagation cost")
        .result
        .expect("cost result");

    assert_eq!(result["peer"].as_str(), Some(peer));
    assert_eq!(result["target_cost"].as_u64(), Some(27));
    assert_eq!(result["source"].as_str(), Some("cached_announce"));
}

#[test]
fn outbound_propagation_cost_accepts_destination_hash_alias() {
    let daemon = RpcDaemon::test_instance();
    let peer = "d4555d7a11368e3d0f568b013286c142";
    let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::from(1_700_000_702i64),
        rmpv::Value::Boolean(true),
        rmpv::Value::from(256),
        rmpv::Value::from(10240),
        rmpv::Value::Array(vec![rmpv::Value::from(8), rmpv::Value::from(3), rmpv::Value::from(8)]),
        rmpv::Value::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");
    daemon
        .handle_rpc(rpc_request(
            97,
            "announce_received",
            json!({
                "peer": peer,
                "timestamp": 1_700_000_702i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("propagation announce");

    let result = daemon
        .handle_rpc(rpc_request(
            98,
            "get_outbound_propagation_cost",
            json!({ "destination_hash": peer }),
        ))
        .expect("get outbound propagation cost")
        .result
        .expect("cost result");

    assert_eq!(result["peer"].as_str(), Some(peer));
    assert_eq!(result["target_cost"].as_u64(), Some(8));
    assert_eq!(result["source"].as_str(), Some("cached_announce"));
}

#[test]
fn outbound_propagation_cost_reports_unavailable_without_silent_default() {
    let daemon = RpcDaemon::test_instance();
    let missing_peer = "11223344556677889900aabbccddeeff";
    daemon
        .handle_rpc(rpc_request(
            95,
            "set_outbound_propagation_node",
            json!({ "peer": missing_peer }),
        ))
        .expect("select propagation node");

    let result = daemon
        .handle_rpc(RpcRequest {
            id: 96,
            method: "get_outbound_propagation_cost".to_string(),
            params: None,
        })
        .expect("get unavailable outbound propagation cost")
        .result
        .expect("cost result");

    assert_eq!(result["peer"].as_str(), Some(missing_peer));
    assert_eq!(result["target_cost"], JsonValue::Null);
    assert_eq!(result["source"].as_str(), Some("unavailable"));
}

#[test]
fn rejected_selected_propagation_node_does_not_update_selection() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            75,
            "propagation_enable",
            json!({
                "enabled": true,
                "max_peers": 1,
            }),
        ))
        .expect("enable propagation");
    daemon
        .handle_rpc(rpc_request(76, "peer_sync", json!({ "peer": "peer-capacity-a" })))
        .expect("fill peer capacity");

    let rejected = daemon
        .handle_rpc(rpc_request(
            77,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-capacity-b" }),
        ))
        .expect_err("selected node should respect peer admission");
    assert!(
        rejected.to_string().contains("max_peers=1"),
        "unexpected rejection error: {rejected}"
    );

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 78,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let nodes = daemon
        .handle_rpc(RpcRequest {
            id: 79,
            method: "list_propagation_nodes".to_string(),
            params: None,
        })
        .expect("list propagation nodes")
        .result
        .expect("list propagation nodes result");
    assert!(
        nodes["nodes"].as_array().expect("propagation nodes").is_empty(),
        "rejected selected node should not be listed"
    );
}

#[test]
fn rejected_propagation_remote_sync_does_not_call_bridge_or_update_lifecycle() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_enable",
            json!({
                "enabled": true,
                "max_peers": 1,
            }),
        ))
        .expect("enable propagation");
    daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-capacity-a" })))
        .expect("fill peer capacity");
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::clone(&sync_calls),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_sync",
            json!({
                "remote": "remote-capacity",
                "peer": "peer-capacity-b",
            }),
        ))
        .expect_err("remote sync should respect peer admission");
    assert!(
        rejected.to_string().contains("max_peers=1"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(sync_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let status = daemon
        .handle_rpc(RpcRequest { id: 83, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x00));
    assert_ne!(propagation["state_name"].as_str(), Some("syncing"));
    assert_ne!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["last_sync_started"], JsonValue::Null);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 84, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-capacity-b")),
        "rejected remote sync peer should not be listed"
    );
}

#[test]
fn propagation_remote_sync_rejects_blank_peer_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::clone(&sync_calls),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            85,
            "propagation_remote_sync",
            json!({
                "remote": "remote-blank-peer",
                "peer": "   ",
            }),
        ))
        .expect_err("blank remote-sync peer should be rejected");
    assert!(
        rejected.to_string().contains("peer is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(sync_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 86, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"].as_array().expect("peer rows").is_empty(),
        "blank remote-sync peer should not create a peer record"
    );
}

#[test]
fn propagation_remote_sync_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::clone(&sync_calls),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_remote_sync",
            json!({
                "remote": "   ",
                "peer": "peer-blank-remote",
            }),
        ))
        .expect_err("blank remote-sync remote should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(sync_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 88, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"].as_array().expect("peer rows").is_empty(),
        "blank remote-sync remote should not create a peer record"
    );
}

#[test]
fn propagation_remote_sync_trims_peer_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_remote_sync",
            json!({
                "remote": "remote-trim-peer",
                "peer": "  peer-trimmed  ",
            }),
        ))
        .expect("remote sync with padded peer")
        .result
        .expect("remote sync result");

    assert_eq!(result["peer"].as_str(), Some("peer-trimmed"));
    assert_eq!(result["result"]["peer"].as_str(), Some("peer-trimmed"));
    assert_eq!(result["peer_sync"]["peer"].as_str(), Some("peer-trimmed"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 88, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-trimmed")));
    assert!(rows
        .iter()
        .all(|row| row["peer"].as_str() != Some("  peer-trimmed  ")));
}

#[test]
fn propagation_remote_sync_trims_remote_before_bridge_event_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            89,
            "propagation_remote_sync",
            json!({
                "remote": "  remote-trimmed  ",
                "peer": "peer-trim-remote",
            }),
        ))
        .expect("remote sync with padded remote")
        .result
        .expect("remote sync result");

    assert_eq!(result["remote"].as_str(), Some("remote-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-trimmed"));
    assert_eq!(result["peer_sync"]["remote"].as_str(), Some("remote-trimmed"));
}

#[test]
fn propagation_remote_sync_uses_stored_peer_case_for_bridge_and_response_like_python() {
    let stored_peer = "Ab".repeat(16);
    let request_peer = stored_peer.to_ascii_lowercase();
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));
    daemon
        .handle_rpc(rpc_request(89, "peer_sync", json!({ "peer": stored_peer.as_str() })))
        .expect("seed mixed-case peer");

    let result = daemon
        .handle_rpc(rpc_request(
            90,
            "propagation_remote_sync",
            json!({
                "remote": "remote-case-peer",
                "peer": request_peer.as_str(),
            }),
        ))
        .expect("remote sync with case-variant peer")
        .result
        .expect("remote sync result");

    assert_eq!(result["peer"].as_str(), Some(stored_peer.as_str()));
    assert_eq!(result["result"]["peer"].as_str(), Some(stored_peer.as_str()));
    assert_eq!(result["peer_sync"]["peer"].as_str(), Some(stored_peer.as_str()));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 91, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some(stored_peer.as_str())));
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some(request_peer.as_str())));
}
