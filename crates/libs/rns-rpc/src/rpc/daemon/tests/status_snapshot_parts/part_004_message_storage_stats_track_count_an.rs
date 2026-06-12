#[test]
fn message_storage_stats_track_count_and_bytes() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            30,
            "propagation_enable",
            json!({
                "enabled": true,
                "message_storage_limit_mb": 4,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_inbound(MessageRecord {
            id: "msg-1".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "hello".to_string(),
            content: "world".to_string(),
            timestamp: 1_700_000_000,
            direction: "in".to_string(),
            fields: Some(json!({"k":"v"})),
            receipt_status: None,
        })
        .expect("store inbound");

    let (count, bytes) = daemon.message_storage_stats().expect("storage stats");
    assert_eq!(count, 1);
    assert!(bytes > 0);

    let result = daemon
        .handle_rpc(RpcRequest { id: 31, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(result["message_count"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["message_storage_limit_mb"].as_u64(), Some(4));
}

#[test]
fn propagation_message_storage_zero_limit_disables_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            32,
            "propagation_enable",
            json!({
                "enabled": true,
                "message_storage_limit_mb": 4,
            }),
        ))
        .expect("enable propagation");
    daemon
        .handle_rpc(rpc_request(
            33,
            "propagation_enable",
            json!({
                "enabled": true,
                "message_storage_limit_mb": 0,
            }),
        ))
        .expect("clear propagation storage limit");

    let result = daemon
        .handle_rpc(RpcRequest { id: 34, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(result["propagation"]["message_storage_limit_mb"], JsonValue::Null);
}

#[test]
fn duplicate_inbound_message_does_not_replace_existing_record_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "duplicate-inbound".to_string(),
            source: "src-a".to_string(),
            destination: "dst".to_string(),
            title: "original title".to_string(),
            content: "original content".to_string(),
            timestamp: 1_700_000_000,
            direction: "in".to_string(),
            fields: Some(json!({"version": 1})),
            receipt_status: None,
        })
        .expect("store original inbound");
    daemon
        .accept_inbound(MessageRecord {
            id: "duplicate-inbound".to_string(),
            source: "src-b".to_string(),
            destination: "dst".to_string(),
            title: "replacement title".to_string(),
            content: "replacement content".to_string(),
            timestamp: 1_700_000_001,
            direction: "in".to_string(),
            fields: Some(json!({"version": 2})),
            receipt_status: None,
        })
        .expect("ignore duplicate inbound");

    let result = daemon
        .handle_rpc(RpcRequest { id: 35, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("list messages result");
    let messages = result["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["source"].as_str(), Some("src-a"));
    assert_eq!(messages[0]["title"].as_str(), Some("original title"));
    assert_eq!(messages[0]["content"].as_str(), Some("original content"));
    assert_eq!(messages[0]["fields"]["version"].as_u64(), Some(1));
}

#[test]
fn list_messages_cursor_paginates_same_second_records_by_id() {
    let daemon = RpcDaemon::test_instance();
    for id in ["msg-a", "msg-c", "msg-b"] {
        daemon
            .accept_inbound(MessageRecord {
                id: id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: id.to_string(),
                content: String::new(),
                timestamp: 1_700_000_100,
                direction: "in".to_string(),
                fields: None,
                receipt_status: None,
            })
            .expect("store same-second message");
    }

    let first = daemon
        .handle_rpc(rpc_request(36, "list_messages", json!({ "limit": 2 })))
        .expect("list first page")
        .result
        .expect("first page result");
    let first_messages = first["messages"].as_array().expect("first messages");
    assert_eq!(
        first_messages.iter().map(|row| row["id"].as_str().unwrap()).collect::<Vec<_>>(),
        vec!["msg-c", "msg-b"]
    );
    assert_eq!(first["next_cursor"].as_str(), Some("1700000100:msg-b"));

    let second = daemon
        .handle_rpc(rpc_request(
            37,
            "list_messages",
            json!({ "cursor": first["next_cursor"].as_str().unwrap(), "limit": 2 }),
        ))
        .expect("list second page")
        .result
        .expect("second page result");
    let second_messages = second["messages"].as_array().expect("second messages");
    assert_eq!(
        second_messages.iter().map(|row| row["id"].as_str().unwrap()).collect::<Vec<_>>(),
        vec!["msg-a"]
    );
    assert_eq!(second["next_cursor"], JsonValue::Null);
}

#[test]
fn list_messages_omits_next_cursor_when_exact_limit_is_exhausted() {
    let daemon = RpcDaemon::test_instance();
    for id in ["msg-a", "msg-b"] {
        daemon
            .accept_inbound(MessageRecord {
                id: id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: id.to_string(),
                content: String::new(),
                timestamp: 1_700_000_101,
                direction: "in".to_string(),
                fields: None,
                receipt_status: None,
            })
            .expect("store exact-limit message");
    }

    let result = daemon
        .handle_rpc(rpc_request(38, "list_messages", json!({ "limit": 2 })))
        .expect("list exact page")
        .result
        .expect("exact page result");

    assert_eq!(result["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(result["next_cursor"], JsonValue::Null);
}

#[test]
fn autopeer_disabled_keeps_announced_peer_unpeered() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            40,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": false,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_010,
            Some("Peer Auto".to_string()),
            Some("announce".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 41, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));

    let status = daemon
        .handle_rpc(RpcRequest { id: 42, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["propagation"]["autopeer"].as_bool(), Some(false));
    assert_eq!(status["propagation"]["autopeer_maxdepth"].as_u64(), Some(2));
}

#[test]
fn announce_received_honors_hops_for_autopeer_maxdepth() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            42,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable autopeer");

    let announce = daemon
        .handle_rpc(rpc_request(
            43,
            "announce_received",
            json!({
                "peer": "peer-too-deep-rpc",
                "timestamp": 1_700_000_109i64,
                "capabilities": ["propagation"],
                "aspect": "lxmf.propagation",
                "hops": 3,
                "interface": "if-auto",
                "source_private_key": "source-private",
                "source_identity": "source-identity",
                "source_node": "source-node",
            }),
        ))
        .expect("announce received")
        .result
        .expect("announce result");
    assert_eq!(announce["peer"], JsonValue::Null);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 44, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "announce_received")
        .cloned()
        .expect("announce event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-too-deep-rpc"));
    assert_eq!(event.payload["hops"].as_u64(), Some(3));
    assert_eq!(event.payload["interface"].as_str(), Some("if-auto"));
    assert_eq!(event.payload["source_private_key"].as_str(), Some("source-private"));
    assert_eq!(event.payload["source_identity"].as_str(), Some("source-identity"));
    assert_eq!(event.payload["source_node"].as_str(), Some("source-node"));
}

#[test]
fn propagation_enable_autopeer_false_unpeers_existing_autopeers() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            43,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable autopeer");

    let entry = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "14".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_108,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-disabled".to_string(),
            1_700_000_109,
            Some("Auto Disabled Peer".to_string()),
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
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept autopeer announce");
    daemon
        .handle_rpc(rpc_request(
            44,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto-disabled" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .handle_rpc(rpc_request(
            45,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": false,
            }),
        ))
        .expect("disable autopeer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 46, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-disabled")));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-disabled")
            .expect("autopeer marks after disabling autopeer")
            .is_empty(),
        "disabling autopeer should clear autopeer queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("autopeer disabled removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-disabled"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("autopeer_disabled"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(0));
    assert_eq!(event.payload["incoming"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(0));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 47,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}
