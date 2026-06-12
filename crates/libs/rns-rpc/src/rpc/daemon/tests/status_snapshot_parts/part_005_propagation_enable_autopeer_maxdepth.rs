#[test]
fn propagation_enable_autopeer_maxdepth_unpeers_existing_deeper_autopeers() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            48,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 3,
            }),
        ))
        .expect("enable autopeer");

    let entry = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(24),
        received_at: 1_700_000_110,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-too-deep".to_string(),
            1_700_000_111,
            Some("Auto Too Deep Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(4)),
            None,
            Some(3),
            None,
            None,
            None,
            None,
        )
        .expect("accept autopeer announce");
    daemon
        .handle_rpc(rpc_request(
            49,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto-too-deep" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .handle_rpc(rpc_request(
            50,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 1,
            }),
        ))
        .expect("tighten autopeer max depth");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-too-deep")));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-too-deep")
            .expect("autopeer marks after tightening max depth")
            .is_empty(),
        "tightening autopeer max depth should clear autopeer queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("autopeer max-depth removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-too-deep"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("autopeer_maxdepth"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 52,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}

#[test]
fn disabled_propagation_node_announce_unpeers_existing_autopeer() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            53,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable autopeer");
    let entry = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(24),
        received_at: 1_700_000_112,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-node-disabled".to_string(),
            1_700_000_113,
            Some("Auto Node Disabled Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(4)),
            Some("lxmf.propagation".to_string()),
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept enabled autopeer announce");
    daemon
        .handle_rpc(rpc_request(
            54,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto-node-disabled" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    let disabled_app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_114i64),
        MsgPackValue::Boolean(false),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode disabled propagation app data");

    daemon
        .accept_announce_with_metadata(
            "peer-auto-node-disabled".to_string(),
            1_700_000_120,
            Some("Auto Node Disabled Peer".to_string()),
            Some("announce".to_string()),
            Some(hex::encode(disabled_app_data)),
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(4)),
            Some("lxmf.propagation".to_string()),
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept disabled propagation announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-node-disabled")));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-node-disabled")
            .expect("autopeer marks after disabled propagation announce")
            .is_empty(),
        "disabled propagation-node announce should clear autopeer queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("disabled propagation removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-node-disabled"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("propagation_disabled"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 56,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}

#[test]
fn disabled_propagation_announce_unpeers_existing_autopeer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            57,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable autopeer");
    let stored_peer = "Peer-Auto-Disabled-Case";
    let announce_peer = stored_peer.to_ascii_lowercase();
    let entry = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(24),
        received_at: 1_700_000_121,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .accept_announce_with_metadata(
            stored_peer.to_string(),
            1_700_000_122,
            Some("Auto Node Disabled Case".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(4)),
            Some("lxmf.propagation".to_string()),
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept enabled autopeer announce");
    daemon
        .handle_rpc(rpc_request(
            58,
            "set_outbound_propagation_node",
            json!({ "peer": stored_peer }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    let disabled_app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_123i64),
        MsgPackValue::Boolean(false),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode disabled propagation app data");

    daemon
        .accept_announce_with_metadata(
            announce_peer,
            1_700_000_124,
            Some("Auto Node Disabled Case".to_string()),
            Some("announce".to_string()),
            Some(hex::encode(disabled_app_data)),
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(4)),
            Some("lxmf.propagation".to_string()),
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept disabled propagation announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 59, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("autopeer marks after disabled propagation announce")
            .is_empty()
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("disabled propagation removal event");
    assert_eq!(event.payload["peer"].as_str(), Some(stored_peer));
    assert_eq!(event.payload["reason"].as_str(), Some("propagation_disabled"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 60,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}

#[test]
fn announce_received_persists_stamp_cost_in_announce_log() {
    let daemon = RpcDaemon::test_instance();
    let announce = daemon
        .handle_rpc(rpc_request(
            43,
            "announce_received",
            json!({
                "peer": "peer-stamp",
                "timestamp": 1_700_000_011i64,
                "stamp_cost": 21,
                "stamp_cost_flexibility": 4,
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let result = daemon
        .handle_rpc(RpcRequest { id: 44, method: "list_announces".to_string(), params: None })
        .expect("list announces")
        .result
        .expect("list announces result");
    let rows = result["announces"].as_array().expect("announce rows");
    let row = rows.first().expect("announce row");
    assert_eq!(row["peer"].as_str(), Some("peer-stamp"));
    assert_eq!(row["timestamp"].as_i64(), Some(1_700_000_011));
    assert_eq!(row["stamp_cost"].as_u64(), Some(21));
    assert_eq!(row["stamp_cost_flexibility"].as_u64(), Some(4));
}

#[test]
fn list_announces_omits_next_cursor_when_exact_limit_is_exhausted() {
    let daemon = RpcDaemon::test_instance();
    for peer in ["peer-b", "peer-a"] {
        daemon
            .handle_rpc(rpc_request(
                45,
                "announce_received",
                json!({
                    "peer": peer,
                    "timestamp": 1_700_000_015i64,
                    "aspect": "lxmf.delivery",
                }),
            ))
            .expect("announce received");
    }

    let result = daemon
        .handle_rpc(rpc_request(46, "list_announces", json!({ "limit": 2 })))
        .expect("list exact announces")
        .result
        .expect("exact announces result");

    assert_eq!(result["announces"].as_array().map(Vec::len), Some(2));
    assert_eq!(result["next_cursor"], JsonValue::Null);
}
