#[test]
fn daemon_status_ex_reads_cached_status_snapshot() {
    let daemon = RpcDaemon::test_instance();
    daemon.replace_interfaces(vec![InterfaceRecord {
        kind: "tcp_client".to_string(),
        enabled: true,
        host: Some("rmap.world".to_string()),
        port: Some(4242),
        name: Some("primary".to_string()),
        settings: None,
    }]);
    daemon.accept_announce("peer-1".to_string(), 1_700_000_000).expect("announce");

    let delivery = daemon
        .handle_rpc(rpc_request(
            10,
            "set_delivery_policy",
            json!({
                "auth_required": true,
                "allowed_destinations": ["alpha"],
                "denied_destinations": ["beta"],
                "ignored_destinations": ["gamma"],
                "prioritised_destinations": ["delta"],
            }),
        ))
        .expect("set delivery policy");
    assert!(delivery.error.is_none());

    let propagation = daemon
        .handle_rpc(rpc_request(
            11,
            "propagation_enable",
            json!({
                "enabled": true,
                "store_root": "/tmp/propagation",
                "target_cost": 9,
                "stamp_cost_flexibility": 4,
            }),
        ))
        .expect("enable propagation");
    assert!(propagation.error.is_none());

    let stamp = daemon
        .handle_rpc(rpc_request(
            12,
            "stamp_policy_set",
            json!({
                "target_cost": 11,
                "flexibility": 3,
            }),
        ))
        .expect("set stamp policy");
    assert!(stamp.error.is_none());

    let response = daemon
        .handle_rpc(RpcRequest { id: 13, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status");
    let result = response.result.expect("daemon status result");

    assert_eq!(result["peer_count"].as_u64(), Some(1));
    assert_eq!(result["interface_count"].as_u64(), Some(1));
    assert_eq!(result["interfaces"][0]["name"].as_str(), Some("primary"));
    assert_eq!(result["delivery_policy"]["auth_required"].as_bool(), Some(true));
    assert_eq!(result["delivery_policy"]["allowed_destinations"][0].as_str(), Some("alpha"));
    assert_eq!(result["propagation"]["enabled"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["target_cost"].as_u64(), Some(9));
    assert_eq!(result["propagation"]["stamp_cost_flexibility"].as_u64(), Some(4));
    assert_eq!(result["stamp_policy"]["target_cost"].as_u64(), Some(11));
    assert_eq!(result["stamp_policy"]["flexibility"].as_u64(), Some(3));
    assert_eq!(result["stamp_policy"]["enforce"].as_bool(), Some(true));
}

fn make_ready_propagation_peer(daemon: &RpcDaemon, peer_seed: u8) -> String {
    let peer = hex::encode([peer_seed; 16]);
    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_606 + i64::from(peer_seed),
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
        .expect("accept ready propagation peer announce");
    peer
}

fn ready_propagation_peer_daemon(peer_seed: u8) -> (RpcDaemon, String) {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = make_ready_propagation_peer(&daemon, peer_seed);
    (daemon, peer)
}

#[test]
fn propagation_policy_is_reported_and_enforced_for_new_peers() {
    let daemon = RpcDaemon::test_instance();

    let propagation = daemon
        .handle_rpc(rpc_request(
            20,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 9,
                "stamp_cost_flexibility": 5,
                "delivery_limit": 321,
                "propagation_limit": 654,
                "sync_limit": 987,
                "static_peers": ["static-peer"],
                "max_peers": 1,
                "from_static_only": true,
                "peering_cost": 18,
                "remote_peering_cost_max": 26,
            }),
        ))
        .expect("enable propagation");
    assert!(propagation.error.is_none());

    let result = daemon
        .handle_rpc(RpcRequest { id: 21, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(result["propagation"]["static_peers"][0].as_str(), Some("static-peer"));
    assert_eq!(result["propagation"]["stamp_cost_flexibility"].as_u64(), Some(5));
    assert_eq!(result["propagation"]["delivery_limit"].as_u64(), Some(321));
    assert_eq!(result["propagation"]["propagation_limit"].as_u64(), Some(654));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(987));
    assert_eq!(result["propagation"]["max_peers"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["from_static_only"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["peering_cost"].as_u64(), Some(18));
    assert_eq!(result["propagation"]["remote_peering_cost_max"].as_u64(), Some(26));
    assert_eq!(result["propagation"]["message_storage_limit_mb"].as_u64(), None);

    daemon.accept_announce("static-peer".to_string(), 1_700_000_000).expect("static peer accepted");
    daemon
        .accept_announce("dynamic-peer".to_string(), 1_700_000_001)
        .expect("dynamic announce accepted");
    let peers = daemon
        .handle_rpc(RpcRequest { id: 22, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1, "non-static announce should not become a peered node");
    assert_eq!(rows[0]["peer"].as_str(), Some("static-peer"));
}

#[test]
fn propagation_enable_activates_static_peers_like_python() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            23,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static"],
            }),
        ))
        .expect("enable propagation");
    assert!(response.error.is_none());

    let status = daemon
        .handle_rpc(RpcRequest { id: 24, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 25, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["peer"].as_str(), Some("peer-static"));
    assert_eq!(rows[0]["peer_type"].as_str(), Some("static"));
    assert_eq!(rows[0]["type"].as_str(), Some("static"));
    assert_eq!(rows[0]["alive"].as_bool(), Some(false));
    assert_eq!(rows[0]["last_seen"].as_i64(), Some(0));
}

#[test]
fn propagation_enable_normalizes_static_peer_config_for_status_and_type() {
    let daemon = RpcDaemon::test_instance();
    let result = daemon
        .handle_rpc(rpc_request(
            25,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["  peer-static-normalized  ", "peer-static-normalized", ""],
            }),
        ))
        .expect("enable propagation")
        .result
        .expect("enable result");
    assert_eq!(
        result["propagation"]["static_peers"].as_array().expect("static peers"),
        &[json!("peer-static-normalized")]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 26, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-normalized"))
        .expect("normalized static peer row");
    assert_eq!(row["peer_type"].as_str(), Some("static"));
    assert_eq!(row["type"].as_str(), Some("static"));
}

#[test]
fn propagation_enable_partial_update_preserves_static_peer_config_and_type() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            27,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-preserved"],
            }),
        ))
        .expect("enable static peer");

    let updated = daemon
        .handle_rpc(rpc_request(
            28,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "max_peers": 1,
            }),
        ))
        .expect("partial propagation update")
        .result
        .expect("partial update result");
    assert_eq!(
        updated["propagation"]["static_peers"].as_array().expect("static peers"),
        &[json!("peer-static-preserved")]
    );
    assert_eq!(updated["propagation"]["from_static_only"].as_bool(), Some(true));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 29, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-preserved"))
        .expect("static peer row");
    assert_eq!(row["peer_type"].as_str(), Some("static"));
    assert_eq!(row["type"].as_str(), Some("static"));

    let blocked = daemon
        .handle_rpc(rpc_request(30, "peer_sync", json!({ "peer": "peer-non-static" })))
        .expect_err("from_static_only should reject new non-static peers");
    assert!(
        blocked.to_string().contains("from_static_only"),
        "unexpected rejection error: {blocked}"
    );
}

#[test]
fn propagation_enable_clears_selected_node_when_static_policy_rejects_it() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            31,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-old"],
            }),
        ))
        .expect("enable old static peer");
    daemon
        .handle_rpc(rpc_request(
            32,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-static-old" }),
        ))
        .expect("select old static peer");

    daemon
        .handle_rpc(rpc_request(
            33,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-new"],
            }),
        ))
        .expect("replace static peer list");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 34,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 35, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 36, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(daemon_status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn propagation_enable_unpeers_removed_static_peers_when_static_only() {
    let daemon = RpcDaemon::test_instance();
    let entry = PropagationEntryRecord {
        transient_id: "af".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_105,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .handle_rpc(rpc_request(
            37,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-old"],
            }),
        ))
        .expect("enable old static peer");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-static-old", entry.transient_id.as_str())
        .expect("mark old peer unhandled");
    daemon
        .handle_rpc(rpc_request(
            38,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-static-old" }),
        ))
        .expect("select old static peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let update = daemon
        .handle_rpc(rpc_request(
            39,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-new"],
            }),
        ))
        .expect("replace static peer list")
        .result
        .expect("replace static peer result");
    assert_eq!(update["propagation"]["selected_node"], JsonValue::Null);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 40, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-static-old")));
    let new_peer = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-new"))
        .expect("new static peer");
    assert_eq!(new_peer["peer_type"].as_str(), Some("static"));
    assert_eq!(new_peer["type"].as_str(), Some("static"));

    let old_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-static-old")
        .expect("old peer pending propagation");
    assert!(old_pending.is_empty());
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("static-only peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-static-old"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["reason"].as_str(), Some("static_only_policy"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 41, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));
}

#[test]
fn propagation_enable_static_only_unpeers_existing_non_static_peers() {
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
        .expect("enable autopeering");

    let entry = PropagationEntryRecord {
        transient_id: "b0".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_106,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-static-only".to_string(),
            1_700_000_107,
            Some("Auto Peer".to_string()),
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
            43,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto-static-only" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .handle_rpc(rpc_request(
            44,
            "propagation_enable",
            json!({
                "enabled": true,
                "from_static_only": true,
                "static_peers": ["peer-static-allowed"],
            }),
        ))
        .expect("enable static-only policy");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 45, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-static-only")));
    assert!(rows.iter().any(|row| {
        row["peer"].as_str() == Some("peer-static-allowed")
            && row["peer_type"].as_str() == Some("static")
    }));

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-static-only")
            .expect("autopeer marks after static-only")
            .is_empty(),
        "static-only policy should clear non-static peer queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("static-only non-static peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-static-only"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("static_only_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 46,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}

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
}

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

#[test]
fn announce_received_parses_delivery_stamp_cost_from_python_app_data() {
    let daemon = RpcDaemon::test_instance();
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Binary(b"Peer Stamp".to_vec()),
        MsgPackValue::from(22),
    ]))
    .expect("encode app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            45,
            "announce_received",
            json!({
                "peer": "peer-delivery-stamp",
                "timestamp": 1_700_000_012i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    assert_eq!(
        daemon.outbound_stamp_cost_for("peer-delivery-stamp").expect("stamp cost lookup"),
        Some(22)
    );
}

#[test]
fn announce_received_ignores_python_invalid_delivery_stamp_cost_from_app_data() {
    let daemon = RpcDaemon::test_instance();
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Binary(b"Peer Stamp".to_vec()),
        MsgPackValue::from(255),
    ]))
    .expect("encode app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            46,
            "announce_received",
            json!({
                "peer": "peer-invalid-delivery-stamp",
                "timestamp": 1_700_000_012i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    assert_eq!(
        daemon.outbound_stamp_cost_for("peer-invalid-delivery-stamp").expect("stamp cost lookup"),
        None
    );
}

#[test]
fn announce_received_parses_propagation_peer_limits_from_python_app_data() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            46,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_013i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            47,
            "announce_received",
            json!({
                "peer": "peer-propagation-limits",
                "timestamp": 1_700_000_013i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let peers = daemon
        .handle_rpc(RpcRequest { id: 48, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(333));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(999));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(5));
    assert_eq!(row["type"].as_str(), Some("discovered"));
    assert_eq!(row["state"].as_u64(), Some(0));
    assert_eq!(row["sync_strategy"].as_u64(), Some(2));
    assert_eq!(row["ler"].as_u64(), Some(0));
    assert_eq!(row["str"].as_u64(), Some(0));
    assert_eq!(row["last_heard"].as_i64(), Some(1_700_000_013));
    assert_eq!(row["transfer_limit"].as_u64(), Some(333));
    assert_eq!(row["sync_limit"].as_u64(), Some(999));
    assert_eq!(row["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(row["stamp_cost_flexibility"].as_u64(), Some(2));
}

#[test]
fn announce_received_uses_python_propagation_node_timebase_for_peer_state() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            49,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_021i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            50,
            "announce_received",
            json!({
                "peer": "peer-pn-timebase",
                "timestamp": 1_700_000_099i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["last_heard"].as_i64(), Some(1_700_000_099));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_021));
}

#[test]
fn static_peer_path_response_announce_does_not_refresh_existing_peer_state() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            52,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-path-response"],
            }),
        ))
        .expect("enable static peer");
    let initial_app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_022i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(vec![(MsgPackValue::from(1), MsgPackValue::Binary(b"Static PN".to_vec()))]),
    ]))
    .expect("encode initial propagation app data");
    daemon
        .handle_rpc(rpc_request(
            53,
            "announce_received",
            json!({
                "peer": "peer-static-path-response",
                "timestamp": 1_700_000_120i64,
                "app_data_hex": hex::encode(initial_app_data),
                "aspect": "lxmf.propagation",
            }),
        ))
        .expect("initial announce");

    let path_response_app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_123i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(444),
        MsgPackValue::from(1111),
        MsgPackValue::Array(vec![
            MsgPackValue::from(9),
            MsgPackValue::from(3),
            MsgPackValue::from(6),
        ]),
        MsgPackValue::Map(vec![(
            MsgPackValue::from(1),
            MsgPackValue::Binary(b"Path Response PN".to_vec()),
        )]),
    ]))
    .expect("encode path response propagation app data");
    daemon
        .handle_rpc(rpc_request(
            54,
            "announce_received",
            json!({
                "peer": "peer-static-path-response",
                "timestamp": 1_700_000_130i64,
                "app_data_hex": hex::encode(path_response_app_data),
                "aspect": "lxmf.propagation",
                "is_path_response": true,
            }),
        ))
        .expect("path response announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-static-path-response"))
        .expect("static peer row");
    assert_eq!(row["last_heard"].as_i64(), Some(1_700_000_120));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_022));
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(333));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(999));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(5));
    assert_eq!(row["name"].as_str(), Some("Static PN"));
}

#[test]
fn announce_received_parses_propagation_peer_name_from_python_metadata() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            49,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_014i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(vec![(MsgPackValue::from(1), MsgPackValue::Binary(b"PN Alpha".to_vec()))]),
    ]))
    .expect("encode propagation app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            50,
            "announce_received",
            json!({
                "peer": "peer-pn-name",
                "timestamp": 1_700_000_014i64,
                "app_data_hex": hex::encode(app_data),
                "capabilities": ["propagation"],
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-pn-name"));
    assert_eq!(row["name"].as_str(), Some("PN Alpha"));
    assert_eq!(row["name_source"].as_str(), Some("pn_meta"));
}

#[test]
fn ticket_generate_reuses_valid_ticket_for_destination() {
    let daemon = RpcDaemon::test_instance();

    let first = daemon
        .handle_rpc(rpc_request(
            90,
            "ticket_generate",
            json!({
                "destination": "peer-ticket",
            }),
        ))
        .expect("ticket generate")
        .result
        .expect("ticket generate result");
    let second = daemon
        .handle_rpc(rpc_request(
            91,
            "ticket_generate",
            json!({
                "destination": "peer-ticket",
            }),
        ))
        .expect("ticket generate")
        .result
        .expect("ticket generate result");

    assert_eq!(first["destination"].as_str(), Some("peer-ticket"));
    assert_eq!(first["ticket"], second["ticket"]);
    assert_eq!(first["expires_at"], second["expires_at"]);
    assert_eq!(first["ticket"].as_str().map(str::len), Some(32));
    assert_eq!(first["included"], json!(true));
}

#[test]
fn ticket_generate_reuses_persisted_ticket_after_daemon_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("tickets.sqlite");

    let first = {
        let store = MessagesStore::open(db_path.as_path()).expect("open store");
        let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
        daemon
            .handle_rpc(rpc_request(
                96,
                "ticket_generate",
                json!({
                    "destination": "peer-ticket-persisted",
                }),
            ))
            .expect("ticket generate")
            .result
            .expect("ticket generate result")
    };

    let second = {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen store");
        let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
        daemon
            .handle_rpc(rpc_request(
                97,
                "ticket_generate",
                json!({
                    "destination": "peer-ticket-persisted",
                }),
            ))
            .expect("ticket generate")
            .result
            .expect("ticket generate result")
    };

    assert_eq!(first["included"], json!(true));
    assert_eq!(second["included"], json!(true));
    assert_eq!(first["ticket"], second["ticket"]);
    assert_eq!(first["expires_at"], second["expires_at"]);

    let store = MessagesStore::open(db_path.as_path()).expect("reopen store");
    let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
    assert_eq!(daemon.valid_issued_tickets_for("peer-ticket-persisted").len(), 1);
}

#[test]
fn ticket_generate_renews_ticket_inside_renewal_window() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("ticket-renew.sqlite");
    let destination = "peer-ticket-renew";
    let old_ticket = "000102030405060708090a0b0c0d0e0f";
    let expiring_at = now_i64() + RpcDaemon::TICKET_RENEW_SECS - 60;

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open store");
        store.upsert_ticket(destination, old_ticket, expiring_at).expect("seed expiring ticket");
    }

    let store = MessagesStore::open(db_path.as_path()).expect("reopen store");
    let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
    let result = daemon
        .handle_rpc(rpc_request(
            99,
            "ticket_generate",
            json!({
                "destination": destination,
            }),
        ))
        .expect("ticket generate")
        .result
        .expect("ticket generate result");

    assert_eq!(result["included"], json!(true));
    assert_ne!(result["ticket"].as_str(), Some(old_ticket));
    assert!(result["expires_at"].as_i64().is_some_and(|expires_at| expires_at > expiring_at));
}

#[test]
fn ticket_renewal_keeps_old_unexpired_ticket_valid_like_python() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("ticket-renew-valid.sqlite");
    let destination = "peer-ticket-renew-valid";
    let old_ticket = "000102030405060708090a0b0c0d0e0f";
    let expiring_at = now_i64() + RpcDaemon::TICKET_RENEW_SECS - 60;

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open store");
        store.upsert_ticket(destination, old_ticket, expiring_at).expect("seed expiring ticket");
    }

    let store = MessagesStore::open(db_path.as_path()).expect("reopen store");
    let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
    let result = daemon
        .handle_rpc(rpc_request(
            100,
            "ticket_generate",
            json!({
                "destination": destination,
            }),
        ))
        .expect("ticket generate")
        .result
        .expect("ticket generate result");
    let new_ticket = result["ticket"].as_str().expect("new ticket");
    assert_ne!(new_ticket, old_ticket);

    let valid_tickets = daemon.valid_issued_tickets_for(destination);
    let old_ticket_bytes = hex::decode(old_ticket).expect("old ticket hex");
    let new_ticket_bytes = hex::decode(new_ticket).expect("new ticket hex");
    assert_eq!(valid_tickets.len(), 2);
    assert!(valid_tickets.contains(&old_ticket_bytes));
    assert!(valid_tickets.contains(&new_ticket_bytes));
}

#[test]
fn signed_inbound_ticket_is_remembered_for_outbound_reply() {
    let daemon = RpcDaemon::test_instance();
    let expires_at = now_i64() + 3_600;
    let ticket = "00112233445566778899aabbccddeeff";

    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-inbound-1".to_string(),
            source: "peer-ticket-source".to_string(),
            destination: "local".to_string(),
            title: "ticket".to_string(),
            content: "ticket body".to_string(),
            timestamp: now_i64(),
            direction: "in".to_string(),
            fields: Some(json!({
                "12": [expires_at, [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255]],
                "_lxmf": {
                    "signature_valid": true,
                },
            })),
            receipt_status: None,
        })
        .expect("accept inbound");

    let remembered =
        daemon.outbound_ticket_for("peer-ticket-source").expect("outbound ticket").expect("ticket");
    assert_eq!(remembered.ticket, ticket);
    assert_eq!(remembered.expires_at, expires_at);
}

#[test]
fn signed_inbound_ticket_accepts_python_float_expiry() {
    let daemon = RpcDaemon::test_instance();
    let expires_at = now_i64() + 3_600;
    let python_expires_at = expires_at as f64 + 0.25;
    let ticket = "00112233445566778899aabbccddeeff";

    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-inbound-float-expiry".to_string(),
            source: "peer-ticket-source".to_string(),
            destination: "local".to_string(),
            title: "ticket".to_string(),
            content: "ticket body".to_string(),
            timestamp: now_i64(),
            direction: "in".to_string(),
            fields: Some(json!({
                "12": [python_expires_at, [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255]],
                "_lxmf": {
                    "signature_valid": true,
                },
            })),
            receipt_status: None,
        })
        .expect("accept inbound");

    let remembered =
        daemon.outbound_ticket_for("peer-ticket-source").expect("outbound ticket").expect("ticket");
    assert_eq!(remembered.ticket, ticket);
    assert_eq!(remembered.expires_at, expires_at + 1);
}

#[test]
fn unsigned_inbound_ticket_is_not_remembered() {
    let daemon = RpcDaemon::test_instance();
    let expires_at = now_i64() + 3_600;

    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-inbound-unsigned".to_string(),
            source: "peer-ticket-source".to_string(),
            destination: "local".to_string(),
            title: "ticket".to_string(),
            content: "ticket body".to_string(),
            timestamp: now_i64(),
            direction: "in".to_string(),
            fields: Some(json!({
                "12": [expires_at, [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255]],
                "_lxmf": {
                    "signature_valid": false,
                },
            })),
            receipt_status: None,
        })
        .expect("accept inbound");

    assert!(daemon.outbound_ticket_for("peer-ticket-source").expect("outbound ticket").is_none());
}

#[test]
fn inbound_ticket_without_validated_signature_metadata_is_not_remembered_like_python() {
    let daemon = RpcDaemon::test_instance();
    let expires_at = now_i64() + 3_600;

    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-inbound-unknown-signature".to_string(),
            source: "peer-ticket-source".to_string(),
            destination: "local".to_string(),
            title: "ticket".to_string(),
            content: "ticket body".to_string(),
            timestamp: now_i64(),
            direction: "in".to_string(),
            fields: Some(json!({
                "12": [expires_at, [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255]],
                "_lxmf": {
                    "signature_checked": false,
                    "signature_status": "source_identity_unknown",
                },
            })),
            receipt_status: None,
        })
        .expect("accept inbound");

    assert!(daemon.outbound_ticket_for("peer-ticket-source").expect("outbound ticket").is_none());
}

#[test]
fn signed_inbound_ticket_hex_string_is_not_remembered_like_python() {
    let daemon = RpcDaemon::test_instance();
    let expires_at = now_i64() + 3_600;

    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-inbound-hex-string".to_string(),
            source: "peer-ticket-source".to_string(),
            destination: "local".to_string(),
            title: "ticket".to_string(),
            content: "ticket body".to_string(),
            timestamp: now_i64(),
            direction: "in".to_string(),
            fields: Some(json!({
                "12": [expires_at, "00112233445566778899aabbccddeeff"],
                "_lxmf": {
                    "signature_valid": true,
                },
            })),
            receipt_status: None,
        })
        .expect("accept inbound");

    assert!(daemon.outbound_ticket_for("peer-ticket-source").expect("outbound ticket").is_none());
}

#[test]
fn ticket_generate_suppresses_recently_delivered_ticket() {
    let daemon = RpcDaemon::test_instance();

    daemon.mark_ticket_delivered("peer-ticket-recent");

    let result = daemon
        .handle_rpc(rpc_request(
            92,
            "ticket_generate",
            json!({
                "destination": "peer-ticket-recent",
            }),
        ))
        .expect("ticket generate")
        .result
        .expect("ticket generate result");

    assert_eq!(result["destination"].as_str(), Some("peer-ticket-recent"));
    assert_eq!(result["included"], json!(false));
    assert_eq!(result["ticket"], JsonValue::Null);
    assert_eq!(result["expires_at"], JsonValue::Null);
    assert_eq!(result["reason"].as_str(), Some("ticket_interval"));
}

#[test]
fn ticket_generate_suppresses_recent_delivery_after_daemon_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("ticket-deliveries.sqlite");

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open store");
        let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
        daemon.mark_ticket_delivered("peer-ticket-restart-interval");
    }

    let result = {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen store");
        let daemon = RpcDaemon::with_store(store, "ticket-node".to_string());
        daemon
            .handle_rpc(rpc_request(
                98,
                "ticket_generate",
                json!({
                    "destination": "peer-ticket-restart-interval",
                }),
            ))
            .expect("ticket generate")
            .result
            .expect("ticket generate result")
    };

    assert_eq!(result["included"], json!(false));
    assert_eq!(result["reason"].as_str(), Some("ticket_interval"));
}

#[test]
fn delivered_include_ticket_message_starts_ticket_interval() {
    let daemon = RpcDaemon::test_instance();

    daemon
        .handle_rpc(rpc_request(
            93,
            "sdk_send_v2",
            json!({
                "id": "ticket-msg-1",
                "source": "local",
                "destination": "peer-ticket-delivered",
                "title": "ticket",
                "content": "ticket body",
                "method": "direct",
                "include_ticket": true,
            }),
        ))
        .expect("send message");
    daemon
        .handle_rpc(rpc_request(
            94,
            "record_receipt",
            json!({
                "message_id": "ticket-msg-1",
                "status": "delivered",
            }),
        ))
        .expect("record delivery");

    let result = daemon
        .handle_rpc(rpc_request(
            95,
            "ticket_generate",
            json!({
                "destination": "peer-ticket-delivered",
            }),
        ))
        .expect("ticket generate")
        .result
        .expect("ticket generate result");

    assert_eq!(result["included"], json!(false));
    assert_eq!(result["reason"].as_str(), Some("ticket_interval"));
}

#[test]
fn autopeered_announce_records_propagation_peer_state() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            45,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
                "remote_peering_cost_max": 8,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_100i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(512),
        MsgPackValue::from(2048),
        MsgPackValue::Array(vec![
            MsgPackValue::from(4),
            MsgPackValue::from(1),
            MsgPackValue::from(7),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_100,
            Some("Peer Auto".to_string()),
            Some("announce".to_string()),
            Some(hex::encode(app_data)),
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(4),
            Some(Some(1)),
            Some(Some(7)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 46, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-auto"));
    assert_eq!(row["peer_type"].as_str(), Some("auto"));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_100));
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(512));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(2048));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(4));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(1));
    assert_eq!(row["peering_cost"].as_u64(), Some(7));
}

#[test]
fn autopeered_announce_queues_existing_propagation_entries() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            45,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
            }),
        ))
        .expect("enable propagation");
    let entry = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_105,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    daemon
        .accept_announce_with_metadata(
            "peer-auto-queue".to_string(),
            1_700_000_106,
            Some("Peer Auto Queue".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
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
        .handle_rpc(RpcRequest { id: 46, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-auto-queue"))
        .expect("peer row");
    assert_eq!(row["peer_type"].as_str(), Some("auto"));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn autopeer_capacity_rejects_peer_but_preserves_announce() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            47,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "autopeer_maxdepth": 2,
                "max_peers": 1,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto-full-a".to_string(),
            1_700_000_120,
            Some("Peer Auto Full A".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
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
        .expect("accept first autopeer announce");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-full-b".to_string(),
            1_700_000_121,
            Some("Peer Auto Full B".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
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
        .expect("capacity-limited announce should still be accepted");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 48, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let peer_rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(peer_rows.len(), 1);
    assert_eq!(peer_rows[0]["peer"].as_str(), Some("peer-auto-full-a"));

    let announces = daemon
        .handle_rpc(RpcRequest { id: 49, method: "list_announces".to_string(), params: None })
        .expect("list announces")
        .result
        .expect("list announces result");
    let announce_rows = announces["announces"].as_array().expect("announce rows");
    assert!(announce_rows.iter().any(|row| {
        row["peer"].as_str() == Some("peer-auto-full-b")
            && row["name"].as_str() == Some("Peer Auto Full B")
    }));

    let event = std::iter::from_fn(|| daemon.take_event())
        .filter(|event| event.event_type == "announce_received")
        .find(|event| event.payload["peer"].as_str() == Some("peer-auto-full-b"))
        .expect("capacity-limited announce event");
    assert_eq!(event.payload["name"].as_str(), Some("Peer Auto Full B"));
}

#[test]
fn propagation_peer_maintenance_culls_unreachable_non_static_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            41,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "static_peers": ["peer-static-unreachable-retained"],
            }),
        ))
        .expect("enable propagation");

    let stale_entry = PropagationEntryRecord {
        transient_id: "c9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_500,
        size_bytes: 24,
        stamp_value: None,
    };
    let static_entry = PropagationEntryRecord {
        transient_id: "ca".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_501,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&stale_entry).expect("store stale entry");
    daemon.store.upsert_propagation_entry(&static_entry).expect("store static entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-auto-unreachable-cull",
            stale_entry.transient_id.as_str(),
        )
        .expect("mark stale unhandled");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-static-unreachable-retained",
            static_entry.transient_id.as_str(),
        )
        .expect("mark static unhandled");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-unreachable-cull".to_string(),
            1_700_000_510,
            Some("Cull Candidate".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(1),
            Some(Some(0)),
            Some(Some(1)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept autopeer announce");

    let stale_last_seen = now_i64() - (14 * 24 * 60 * 60) - 1;
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let stale = peers.get_mut("peer-auto-unreachable-cull").expect("stale peer");
        stale.last_seen = stale_last_seen;
        stale.alive = false;
        let static_peer =
            peers.get_mut("peer-static-unreachable-retained").expect("static peer");
        static_peer.last_seen = stale_last_seen;
        static_peer.alive = false;
        static_peer.next_sync_attempt = i64::MAX;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(42, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(1));
    assert_eq!(
        result["culled_peers"].as_array().expect("culled peers"),
        &[json!("peer-auto-unreachable-cull")]
    );
    assert_eq!(result["synced_peer"].as_str(), None);
    let peers = daemon
        .handle_rpc(RpcRequest { id: 43, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-unreachable-cull")));
    assert!(
        rows.iter()
            .any(|row| row["peer"].as_str() == Some("peer-static-unreachable-retained"))
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-unreachable-cull")
            .expect("stale unhandled marks")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-static-unreachable-retained")
            .expect("static unhandled marks"),
        vec![static_entry]
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-unreachable-cull"));
    assert_eq!(event.payload["reason"].as_str(), Some("max_unreachable"));
}

#[test]
fn propagation_peer_maintenance_rotates_low_acceptance_autopeers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            44,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "max_peers": 3,
            }),
        ))
        .expect("enable propagation");

    for (peer, timestamp) in [
        ("peer-rotation-low", 1_700_000_610),
        ("peer-rotation-keep-a", 1_700_000_611),
        ("peer-rotation-keep-b", 1_700_000_612),
    ] {
        daemon
            .accept_announce_with_metadata(
                peer.to_string(),
                timestamp,
                Some(peer.to_string()),
                Some("announce".to_string()),
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(0)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept autopeer announce");
    }

    let recent = now_i64();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for peer in [
            "peer-rotation-low",
            "peer-rotation-keep-a",
            "peer-rotation-keep-b",
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.last_seen = recent;
            record.alive = true;
            record.last_sync_attempt = recent - 1;
            record.offered = 10;
        }
        peers.get_mut("peer-rotation-low").expect("low-rate peer").outgoing = 0;
        peers.get_mut("peer-rotation-keep-a").expect("kept peer").outgoing = 9;
        peers.get_mut("peer-rotation-keep-b").expect("kept peer").outgoing = 10;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(45, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["rotated"].as_u64(), Some(1));
    assert_eq!(
        result["rotated_peers"].as_array().expect("rotated peers"),
        &[json!("peer-rotation-low")]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 46, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-rotation-low")));
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-keep-a")));
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-keep-b")));

    let event = std::iter::from_fn(|| daemon.take_event())
        .find(|event| event.event_type == "peer_unpeer")
        .expect("rotation unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-rotation-low"));
    assert_eq!(event.payload["reason"].as_str(), Some("peer_rotation"));
}

#[test]
fn propagation_peer_maintenance_rotates_low_acceptance_non_static_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            47,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "max_peers": 3,
                "static_peers": ["peer-rotation-static"],
            }),
        ))
        .expect("enable propagation");

    daemon
        .handle_rpc(rpc_request(48, "peer_sync", json!({ "peer": "peer-rotation-manual-low" })))
        .expect("create manual peer");
    daemon
        .handle_rpc(rpc_request(49, "peer_sync", json!({ "peer": "peer-rotation-static" })))
        .expect("create static peer");
    daemon
        .accept_announce_with_metadata(
            "peer-rotation-auto-keep".to_string(),
            1_700_000_613,
            Some("peer-rotation-auto-keep".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(1),
            Some(Some(0)),
            Some(Some(1)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept autopeer announce");

    let recent = now_i64();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for (peer, outgoing) in [
            ("peer-rotation-manual-low", 0),
            ("peer-rotation-static", 10),
            ("peer-rotation-auto-keep", 10),
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.last_seen = recent;
            record.alive = true;
            record.last_sync_attempt = recent - 1;
            record.offered = 10;
            record.outgoing = outgoing;
        }
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(50, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["rotated"].as_u64(), Some(1));
    assert_eq!(
        result["rotated_peers"].as_array().expect("rotated peers"),
        &[json!("peer-rotation-manual-low")]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(
        rows.iter().all(|row| row["peer"].as_str() != Some("peer-rotation-manual-low"))
    );
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-static")));
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-auto-keep")));

    let event = std::iter::from_fn(|| daemon.take_event())
        .find(|event| event.event_type == "peer_unpeer")
        .expect("rotation unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-rotation-manual-low"));
    assert_eq!(event.payload["reason"].as_str(), Some("peer_rotation"));
}

#[test]
fn propagation_peer_maintenance_syncs_one_waiting_peer_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x53);
    let entry = PropagationEntryRecord {
        transient_id: "d5".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "24".repeat(32),
        received_at: 1_700_000_618,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = true;
        record.last_seen = now_i64();
        record.last_sync_attempt = record.last_seen.saturating_sub(1);
        record.next_sync_attempt = 0;
        record.sync_transfer_rate = 1024.0;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(52, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["rotated"].as_u64(), Some(0));
    assert_eq!(result["synced_peer"].as_str(), Some(peer.as_str()));
    assert_eq!(result["peer_sync"]["peer"].as_str(), Some(peer.as_str()));
    assert_eq!(result["peer_sync"]["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["peer_sync"]["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![entry.transient_id]
    );

    let event = std::iter::from_fn(|| daemon.take_event())
        .find(|event| event.event_type == "peer_sync")
        .expect("maintenance peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer.as_str()));
    assert_eq!(event.payload["propagation"]["transferred"].as_u64(), Some(1));
}

#[test]
fn propagation_peer_maintenance_candidate_pool_includes_unknown_speed_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    let fast_peer = make_ready_propagation_peer(&daemon, 0x54);
    let slower_peer = make_ready_propagation_peer(&daemon, 0x55);
    let unknown_speed_peer = make_ready_propagation_peer(&daemon, 0x56);
    let entry = PropagationEntryRecord {
        transient_id: "d7".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "26".repeat(32),
        received_at: 1_700_000_620,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [&fast_peer, &slower_peer, &unknown_speed_peer] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for (peer, rate) in [
            (fast_peer.as_str(), 2_048.0),
            (slower_peer.as_str(), 1_024.0),
            (unknown_speed_peer.as_str(), 0.0),
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.alive = true;
            record.last_seen = 1_700_000_621;
            record.last_sync_attempt = record.last_seen.saturating_sub(1);
            record.next_sync_attempt = 0;
            record.sync_transfer_rate = rate;
        }
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_621)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(unknown_speed_peer.as_str()));
}

#[test]
fn propagation_peer_maintenance_candidate_pool_includes_all_unknown_speed_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    let fast_peer = make_ready_propagation_peer(&daemon, 0x5a);
    let slower_peer = make_ready_propagation_peer(&daemon, 0x5b);
    let first_unknown_peer = make_ready_propagation_peer(&daemon, 0x5c);
    let second_unknown_peer = make_ready_propagation_peer(&daemon, 0x5d);
    let third_unknown_peer = make_ready_propagation_peer(&daemon, 0x5e);
    let entry = PropagationEntryRecord {
        transient_id: "da".repeat(32),
        destination: "1c".repeat(16),
        payload_hex: "28".repeat(32),
        received_at: 1_700_000_624,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [
        &fast_peer,
        &slower_peer,
        &first_unknown_peer,
        &second_unknown_peer,
        &third_unknown_peer,
    ] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for (peer, rate) in [
            (fast_peer.as_str(), 2_048.0),
            (slower_peer.as_str(), 1_024.0),
            (first_unknown_peer.as_str(), 0.0),
            (second_unknown_peer.as_str(), 0.0),
            (third_unknown_peer.as_str(), 0.0),
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.alive = true;
            record.last_seen = 1_700_000_624;
            record.last_sync_attempt = record.last_seen.saturating_sub(1);
            record.next_sync_attempt = 0;
            record.sync_transfer_rate = rate;
        }
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_624)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(third_unknown_peer.as_str()));
}

#[test]
fn propagation_peer_maintenance_unresponsive_pool_does_not_starve_later_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    let first_peer = make_ready_propagation_peer(&daemon, 0x57);
    let second_peer = make_ready_propagation_peer(&daemon, 0x58);
    let third_peer = make_ready_propagation_peer(&daemon, 0x59);
    let entry = PropagationEntryRecord {
        transient_id: "d8".repeat(32),
        destination: "1b".repeat(16),
        payload_hex: "27".repeat(32),
        received_at: 1_700_000_621,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [&first_peer, &second_peer, &third_peer] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for peer in [&first_peer, &second_peer, &third_peer] {
            let record = peers.get_mut(peer.as_str()).expect("peer record");
            record.alive = false;
            record.last_seen = 1_700_000_621;
            record.last_sync_attempt = record.last_seen.saturating_sub(1);
            record.next_sync_attempt = 0;
        }
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_623)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(second_peer.as_str()));
}

#[test]
fn propagation_peer_maintenance_does_not_sync_unreachable_static_peer_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            53,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-unreachable-sync-skip"],
            }),
        ))
        .expect("enable propagation");

    let entry = PropagationEntryRecord {
        transient_id: "d6".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "25".repeat(32),
        received_at: 1_700_000_619,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-static-unreachable-sync-skip",
            entry.transient_id.as_str(),
        )
        .expect("mark unhandled");
    daemon
        .accept_announce_with_metadata(
            "peer-static-unreachable-sync-skip".to_string(),
            1_700_000_619,
            Some("Static Unreachable".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(1),
            Some(Some(0)),
            Some(Some(1)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept static peer announce");
    let stale_last_seen = now_i64() - (14 * 24 * 60 * 60) - 1;
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers
            .get_mut("peer-static-unreachable-sync-skip")
            .expect("static peer");
        record.alive = false;
        record.last_seen = stale_last_seen;
        record.next_sync_attempt = 0;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(54, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["synced_peer"].as_str(), None);
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-static-unreachable-sync-skip")
            .expect("list unhandled"),
        vec![entry]
    );
    assert!(
        std::iter::from_fn(|| daemon.take_event()).all(|event| event.event_type != "peer_sync")
    );
}

#[test]
fn stale_announce_does_not_regress_propagation_peer_state() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            47,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_200,
            Some("New Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(5),
            Some(Some(2)),
            Some(Some(6)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept fresh announce");
    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_150,
            Some("Old Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(2),
            Some(Some(0)),
            Some(Some(3)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept stale announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 48, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["name"].as_str(), Some("New Peer"));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_200));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(5));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(6));

    let announces = daemon
        .handle_rpc(RpcRequest { id: 49, method: "list_announces".to_string(), params: None })
        .expect("list announces")
        .result
        .expect("list announces result");
    let rows = announces["announces"].as_array().expect("announce rows");
    assert_eq!(rows.first().and_then(|row| row["timestamp"].as_i64()), Some(1_700_000_200));
    assert_eq!(rows.get(1).and_then(|row| row["timestamp"].as_i64()), Some(1_700_000_150));
}

#[test]
fn equal_timebase_announce_does_not_refresh_propagation_peer_state_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            49,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto-equal-timebase".to_string(),
            1_700_000_210,
            Some("Equal Timebase Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(5),
            Some(Some(2)),
            Some(Some(6)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept initial announce");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-equal-timebase".to_string(),
            1_700_000_210,
            Some("Equal Timebase Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(2),
            Some(Some(0)),
            Some(Some(3)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept equal-timebase announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 50, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_210));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(5));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(6));
}

#[test]
fn discovered_announce_bursts_do_not_collapse_in_announce_log() {
    let daemon = RpcDaemon::test_instance();
    let timestamp = 1_700_000_250;

    for idx in 0..4 {
        daemon
            .accept_announce_with_metadata(
                "peer-discovered".to_string(),
                timestamp,
                Some(format!("Peer Discovered {idx}")),
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
            .expect("accept discovered announce");
    }

    let announces = daemon
        .handle_rpc(RpcRequest { id: 50, method: "list_announces".to_string(), params: None })
        .expect("list announces")
        .result
        .expect("list announces result");
    let rows = announces["announces"].as_array().expect("announce rows");
    let matching = rows
        .iter()
        .filter(|row| row["peer"].as_str() == Some("peer-discovered"))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 4, "same-second discovered announces must remain distinct");
    let unique_ids = matching
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_ids.len(), 4, "announce log IDs must be unique for burst traffic");

    let legacy_event_ids = std::iter::from_fn(|| daemon.take_event())
        .filter(|event| event.event_type == "announce_received")
        .filter_map(|event| event.payload["id"].as_str().map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        legacy_event_ids.len(),
        4,
        "daemon event queue must expose unique announce IDs for burst traffic"
    );

    let events = daemon
        .handle_rpc(rpc_request(51, "sdk_poll_events_v2", json!({ "cursor": null, "max": 20 })))
        .expect("poll sdk events")
        .result
        .expect("sdk events result");
    let event_rows = events["events"].as_array().expect("event rows");
    let announce_event_ids = event_rows
        .iter()
        .filter(|row| row["event_type"].as_str() == Some("announce_received"))
        .filter_map(|row| row["payload"]["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        announce_event_ids.len(),
        4,
        "SDK announce events must expose unique IDs for burst traffic"
    );
}

#[test]
fn peering_cost_policy_blocks_and_breaks_autopeers() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            50,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_299,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_300,
            None,
            None,
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
        .expect("accept initial announce");
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto")
            .expect("queued autopeer propagation"),
        vec![entry.clone()]
    );
    daemon
        .handle_rpc(rpc_request(
            51,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-auto" }),
        ))
        .expect("select autopeer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_301,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(9)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept high-cost announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto")
            .expect("autopeer propagation marks after break")
            .is_empty(),
        "breaking an autopeer should clear stale propagation queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("autopeer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("peering_cost_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 53,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);
}

#[test]
fn peer_activity_updates_runtime_counters() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            50,
            "peer_sync",
            json!({
                "peer": "peer-runtime",
            }),
        ))
        .expect("peer sync");

    daemon.record_inbound_peer_activity("peer-runtime", 120);
    daemon.record_outbound_peer_activity("peer-runtime", 80, true);
    daemon.record_outbound_peer_activity("peer-runtime", 40, false);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-runtime"));
    assert_eq!(row["rx_bytes"].as_u64(), Some(120));
    assert_eq!(row["tx_bytes"].as_u64(), Some(120));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(
        row["next_sync_attempt"].as_i64(),
        Some(row["last_sync_attempt"].as_i64().expect("last sync attempt") + 12 * 60)
    );
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 1.0));
}

#[test]
fn delivered_peer_activity_updates_last_heard_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(51, "peer_sync", json!({ "peer": "peer-delivered-heard" })))
        .expect("peer sync");

    daemon.record_outbound_peer_activity("peer-delivered-heard", 64, true);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    let last_seen = row["last_seen"].as_i64().expect("last_seen");
    assert!(last_seen > 0);
    assert_eq!(row["last_heard"].as_i64(), Some(last_seen));
    assert_eq!(row["last_sync_attempt"].as_i64(), Some(last_seen));
}

#[test]
fn sent_peer_activity_does_not_mark_peer_heard_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            51,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-sent-only"],
            }),
        ))
        .expect("enable static peer");

    daemon.record_outbound_peer_sent("peer-sent-only", 64);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 52, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-sent-only"));
    assert_eq!(row["tx_bytes"].as_u64(), Some(64));
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["last_heard"].as_i64(), Some(0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
    assert!(row["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
}

#[test]
fn failed_peer_activity_does_not_mark_unheard_static_peer_alive() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            52,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-failed"],
            }),
        ))
        .expect("enable static peer");

    daemon.record_outbound_peer_activity("peer-static-failed", 32, false);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-static-failed"));
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["last_heard"].as_i64(), Some(0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
}

#[test]
fn new_peer_acceptance_rate_matches_python_zero_offer_default() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-zero-offers" })))
        .expect("peer sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-zero-offers"));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
}

#[test]
fn list_peers_reports_zero_acceptance_rate_when_no_offers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-derived-rate" })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-derived-rate").expect("peer record");
        peer.acceptance_rate = 0.9;
    }

    let peers = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-derived-rate"));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
}

#[test]
fn list_peers_preserves_python_peer_message_counters() {
    let daemon = RpcDaemon::test_instance();
    let peer: PeerRecord = serde_json::from_value(json!({
        "peer": "peer-python-counters",
        "last_seen": 1_700_001_100,
        "offered": 7,
        "outgoing": 5,
        "incoming": 3,
        "acceptance_rate": 0.9,
    }))
    .expect("deserialize python counter peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.peer.clone(), peer);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-python-counters"))
        .expect("peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(7));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(5));
    assert_eq!(row["messages"]["incoming"].as_u64(), Some(3));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(5.0 / 7.0));
}

#[test]
fn announce_received_returns_enriched_peer_accounting_like_list_peers() {
    let daemon = RpcDaemon::test_instance();
    let peer: PeerRecord = serde_json::from_value(json!({
        "peer": "peer-announce-accounting",
        "last_seen": 1_700_001_100,
        "offered": 7,
        "outgoing": 5,
        "incoming": 3,
        "acceptance_rate": 0.9,
    }))
    .expect("deserialize python counter peer");
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.peer.clone(), peer);

    let response = daemon
        .handle_rpc(rpc_request(
            55,
            "announce_received",
            json!({
                "peer": "peer-announce-accounting",
                "timestamp": 1_700_001_200,
                "name": "Announced Accounting Peer",
                "name_source": "test",
            }),
        ))
        .expect("announce received")
        .result
        .expect("announce result");
    let peer = &response["peer"];

    assert_eq!(peer["peer"].as_str(), Some("peer-announce-accounting"));
    assert_eq!(peer["messages"]["offered"].as_u64(), Some(7));
    assert_eq!(peer["messages"]["outgoing"].as_u64(), Some(5));
    assert_eq!(peer["messages"]["incoming"].as_u64(), Some(3));
    assert_eq!(peer["offered"].as_u64(), Some(7));
    assert_eq!(peer["outgoing"].as_u64(), Some(5));
    assert_eq!(peer["incoming"].as_u64(), Some(3));
    assert_eq!(peer["acceptance_rate"].as_f64(), Some(5.0 / 7.0));
}

#[test]
fn peer_sync_without_offers_preserves_failure_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-backoff-no-offers" })))
        .expect("initial peer sync");
    daemon.record_outbound_peer_activity("peer-backoff-no-offers", 64, false);

    let before = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let before_row = before["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-backoff-no-offers"))
        .expect("peer row");
    let sync_backoff = before_row["sync_backoff"].as_u64().expect("sync backoff");
    let next_sync_attempt =
        before_row["next_sync_attempt"].as_i64().expect("next sync attempt");
    assert!(sync_backoff > 0);
    assert!(next_sync_attempt > 0);

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-backoff-no-offers" })))
        .expect("no-offer peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));

    let after = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let after_row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-backoff-no-offers"))
        .expect("peer row");
    assert_eq!(after_row["sync_backoff"].as_u64(), Some(sync_backoff));
    assert_eq!(after_row["next_sync_attempt"].as_i64(), Some(next_sync_attempt));
    assert_eq!(after_row["alive"].as_bool(), Some(false));
}

#[test]
fn peer_sync_without_offers_preserves_liveness_when_not_backing_off_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-empty-alive" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-empty-alive").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-empty-alive" })))
        .expect("empty peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.5));
    let last_sync_attempt = result["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-empty-alive"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("empty peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-empty-alive"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
}

#[test]
fn peer_sync_during_backoff_postpones_skipped_offers() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x47);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
        record.peering_timebase = 1_700_000_000;
        record.network_distance = 3;
    }
    let previous_transfer = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "19".repeat(40),
        received_at: 1_700_000_613,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&previous_transfer).expect("store previous transfer");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            previous_transfer.transient_id.as_str(),
        )
        .expect("mark previous transfer unhandled");
    daemon
        .handle_rpc(rpc_request(
            53,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with previous transfer");
    let previous_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x19; 40]])).expect("pack sync resource").len();
    daemon.record_outbound_peer_activity(peer.as_str(), 64, false);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(24);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_614,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let before = daemon
        .handle_rpc(RpcRequest { id: 53, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let before_row = before["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    let sync_backoff = before_row["sync_backoff"].as_u64().expect("sync backoff");
    let next_sync_attempt =
        before_row["next_sync_attempt"].as_i64().expect("next sync attempt");
    assert!(sync_backoff > 0);
    assert!(next_sync_attempt > 0);

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("skipped peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["state"].as_u64(), Some(0));
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["ler"].as_u64(), Some(0));
    assert_eq!(result["network_distance"].as_u64(), Some(3));
    assert_eq!(result["peering_timebase"].as_i64(), Some(1_700_000_000));
    assert_eq!(result["rx_bytes"].as_u64(), Some(0));
    assert_eq!(result["tx_bytes"].as_u64(), Some((previous_resource_bytes + 64) as u64));
    assert_eq!(result["alive"].as_bool(), Some(false));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(previous_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(previous_resource_bytes as u64));
    assert!(result["last_heard"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(result["propagation"]["synced"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer limited ids"),
        &[] as &[JsonValue]
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["postponed"].as_bool(), Some(true));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));

    let after = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let after_row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(after_row["sync_backoff"].as_u64(), Some(sync_backoff));
    assert_eq!(after_row["next_sync_attempt"].as_i64(), Some(next_sync_attempt));
    assert_eq!(after_row["sync_transfer_rate"].as_f64(), Some(previous_resource_bytes as f64));
    assert_eq!(after_row["str"].as_u64(), Some(previous_resource_bytes as u64));
}

#[test]
fn peer_sync_during_backoff_does_not_queue_new_existing_entries_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-backoff-no-queue" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-backoff-no-queue").expect("peer record");
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = now_i64().saturating_add(12 * 60);
    }
    let entry = PropagationEntryRecord {
        transient_id: "e8".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_615,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-backoff-no-queue" })))
        .expect("backoff peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-backoff-no-queue")
            .expect("pending propagation")
            .is_empty()
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-backoff-no-queue")
            .expect("handled ids")
            .is_empty()
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-backoff-no-queue"))
        .expect("peer row");
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
}

#[test]
fn peer_sync_postpones_offers_until_stamp_policy_is_known() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-missing-stamp-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-missing-stamp-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "eb".repeat(32),
        destination: "1b".repeat(16),
        payload_hex: "1b".repeat(20),
        received_at: 1_700_000_617,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-missing-stamp-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-missing-stamp-policy" })))
        .expect("policy-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["synced"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-missing-stamp-policy")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_stamp_policy_postpone_preserves_existing_liveness_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-policy-live" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-policy-live").expect("peer record");
        peer.alive = true;
        peer.last_seen = 1;
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(20),
        received_at: 1_700_000_621,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-policy-live", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-policy-live" })))
        .expect("policy-gated peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["alive"].as_bool(), Some(true));

    let after = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-policy-live"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
}

#[test]
fn peer_sync_postpones_unstamped_offers_when_peer_stamp_policy_is_partial() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-partial-stamp-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-partial-stamp-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(3);
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_619,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-partial-stamp-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-partial-stamp-policy" })))
        .expect("policy-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-partial-stamp-policy")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_postpones_unstamped_offers_until_stamp_policy_is_known() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-unknown-stamp-policy" })))
        .expect("initial peer sync");
    let entry = PropagationEntryRecord {
        transient_id: "e9".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(20),
        received_at: 1_700_000_623,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unknown-stamp-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-unknown-stamp-policy" })))
        .expect("policy-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-unknown-stamp-policy")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_requires_stamp_policy_for_ordinary_limited_peer_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-limited-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-limited-policy").expect("peer record");
        peer.propagation_transfer_limit = Some(1_000);
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ea".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "1a".repeat(20),
        received_at: 1_700_000_624,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-limited-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-limited-policy" })))
        .expect("policy-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-limited-policy")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_request_transfer_limit_keeps_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-request-limit-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-request-limit-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "e7".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(20),
        received_at: 1_700_000_625,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-request-limit-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            54,
            "peer_sync",
            json!({
                "peer": "peer-request-limit-policy",
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("policy-gated request-limited peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["transfer_limit"].as_u64(), Some(1_000));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(1_000));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-request-limit-policy")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_postpones_stamped_offers_until_peering_key_is_ready() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-missing-peering-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-missing-peering-key").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ec".repeat(32),
        destination: "1c".repeat(16),
        payload_hex: "1c".repeat(20),
        received_at: 1_700_000_618,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-missing-peering-key", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-missing-peering-key" })))
        .expect("peering-key-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-missing-peering-key")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_transfer_limits_oversized_stamped_entries_before_peering_key_gate() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-key-limit-first" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-key-limit-first").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let oversized = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "1f".repeat(100),
        received_at: 1_700_000_621,
        size_bytes: 100,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-key-limit-first", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-key-limit-first" })))
        .expect("transfer-limited peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["postpone_reason"], JsonValue::Null);
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer limited ids"),
        &[json!(oversized.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-key-limit-first")
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-key-limit-first")
            .expect("handled ids"),
        vec![oversized.transient_id]
    );
}

#[test]
fn peer_sync_sync_limits_stamped_entries_before_peering_key_gate() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-key-sync-limit-first" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-key-sync-limit-first").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let skipped = PropagationEntryRecord {
        transient_id: "ed".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_622,
        size_bytes: 20,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&skipped).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-key-sync-limit-first", skipped.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-key-sync-limit-first" })))
        .expect("sync-limited peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["postpone_reason"], JsonValue::Null);
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["remaining_bytes"].as_u64(), Some(20));
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(skipped.transient_id.as_str())]
    );

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-key-sync-limit-first")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, skipped.transient_id);
}

#[test]
fn peer_sync_postpones_unstamped_offers_until_peering_key_is_ready() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-unstamped-missing-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unstamped-missing-key").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "1e".repeat(20),
        received_at: 1_700_000_620,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unstamped-missing-key", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": "peer-unstamped-missing-key" })))
        .expect("peering-key-gated peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-unstamped-missing-key")
        .expect("list unhandled");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn repeated_skipped_peer_sync_retries_without_failure_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-skipped-repeat" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-skipped-repeat").expect("peer record");
        peer.propagation_sync_limit = Some(24);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ea".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "1a".repeat(20),
        received_at: 1_700_000_616,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-skipped-repeat", entry.transient_id.as_str())
        .expect("mark unhandled");

    daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-skipped-repeat" })))
        .expect("first skipped peer sync");
    let first = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let first_row = first["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-skipped-repeat"))
        .expect("peer row");
    let first_attempt = first_row["last_sync_attempt"].as_i64().expect("first attempt");
    assert_eq!(first_row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(first_row["next_sync_attempt"].as_i64(), Some(0));

    let second_result = daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-skipped-repeat" })))
        .expect("second skipped peer sync")
        .result
        .expect("second peer sync result");
    assert_eq!(second_result["synced"].as_bool(), Some(true));
    assert_eq!(second_result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(second_result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(second_result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(second_result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(second_result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(second_result["next_sync_attempt"].as_i64(), Some(0));

    let second = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let second_row = second["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-skipped-repeat"))
        .expect("peer row");
    assert_eq!(second_row["sync_backoff"].as_u64(), Some(0));
    assert!(second_row["last_sync_attempt"].as_i64().is_some_and(|value| value >= first_attempt));
    assert_eq!(second_row["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_with_only_skipped_offers_clears_failure_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-skipped-initial" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-skipped-initial").expect("peer record");
        peer.propagation_sync_limit = Some(24);
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = 0;
    }
    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(20),
        received_at: 1_700_000_615,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-skipped-initial", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-skipped-initial" })))
        .expect("skipped peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-skipped-initial"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_result_and_event_report_skipped_only_completion() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-backoff-report" })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-backoff-report").expect("peer record");
        peer.propagation_sync_limit = Some(24);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ba".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(20),
        received_at: 1_700_000_618,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-backoff-report", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-backoff-report" })))
        .expect("skipped peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    let last_sync_attempt = result["last_sync_attempt"].as_i64().expect("last sync attempt");
    let last_heard = result["last_heard"].as_i64().expect("last heard");
    assert!(last_sync_attempt > 0);
    assert!(last_heard > 0);
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(event.payload["last_heard"].as_i64(), Some(last_heard));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(event.payload["propagation"]["skipped"].as_u64(), Some(1));
}

#[test]
fn list_peers_exposes_python_style_message_counters() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-message-stats" })))
        .expect("peer sync");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-message-stats-in".to_string(),
            source: "peer-message-stats".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_600,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("accept inbound");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-message-stats-out".to_string(),
            source: "local".to_string(),
            destination: "peer-message-stats".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_601,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-message-stats"))
        .expect("peer row");
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
}

#[test]
fn peer_sync_marks_unhandled_propagation_entries_handled() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x41);
    let entry = PropagationEntryRecord {
        transient_id: "ab".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(16),
        received_at: 1_700_000_605,
        size_bytes: 16,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark propagation unhandled");

    daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("peer sync");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("list handled"),
        vec![entry.transient_id]
    );
}

#[test]
fn peer_sync_queues_existing_entries_for_new_manual_peer() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([0x42_u8; 16]);
    daemon
        .handle_rpc(rpc_request(54, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("seed manual peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_stamp_cost = Some(1);
        record.propagation_stamp_cost_flexibility = Some(1);
        record.peering_cost = Some(1);
    }
    let entry = PropagationEntryRecord {
        transient_id: "ac".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_606,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("manual"));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_offer_response_only_transfers_wanted_messages_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xad);
    let wanted = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "ae".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(30),
        received_at: 1_700_000_608,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["offered"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["offered"].as_u64(), Some(2));
    assert_eq!(result["outgoing"].as_u64(), Some(1));
    assert_eq!(result["incoming"].as_u64(), Some(0));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.5));
    assert_eq!(
        result["propagation"]["messages"].as_array().expect("transferred messages").len(),
        1
    );
    assert_eq!(
        result["propagation"]["messages"][0]["transient_id"].as_str(),
        Some(wanted.transient_id.as_str())
    );

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![wanted.transient_id, already_known.transient_id]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["incoming"].as_u64(), Some(0));
}

#[test]
fn peer_sync_boolean_wanted_ids_true_transfers_all_offered_messages_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([3u8; 16]);
    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_606,
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
        .expect("accept ready propagation peer announce");
    let first = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(30),
        received_at: 1_700_000_608,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer,
                "wanted_ids": true,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(2));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(first.transient_id.as_str()), json!(second.transient_id.as_str())]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_boolean_wanted_ids_true_keeps_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-wants-all-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-wants-all-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "b7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "17".repeat(24),
        received_at: 1_700_000_609,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-wants-all-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-wants-all-policy",
                "wanted_ids": true,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-wants-all-policy")
        .expect("pending propagation");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_selected_wanted_ids_keep_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-selected-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-selected-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "b8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "18".repeat(24),
        received_at: 1_700_000_610,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-selected-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-selected-policy",
                "wanted_ids": [entry.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-selected-policy")
        .expect("pending propagation");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_empty_wanted_ids_keep_full_offer_policy_gates_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-wants-none-policy" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-wants-none-policy").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = None;
        peer.propagation_stamp_cost_flexibility = None;
        peer.peering_cost = None;
    }
    let entry = PropagationEntryRecord {
        transient_id: "b9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "19".repeat(24),
        received_at: 1_700_000_611,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-wants-none-policy", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-wants-none-policy",
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(
        result["propagation"]["postpone_reason"].as_str(),
        Some("stamp_policy")
    );
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));

    let unhandled = daemon
        .store
        .list_peer_unhandled_propagation("peer-wants-none-policy")
        .expect("pending propagation");
    assert_eq!(unhandled.len(), 1);
    assert_eq!(unhandled[0].transient_id, entry.transient_id);
}

#[test]
fn peer_sync_boolean_wanted_ids_false_handles_all_offered_messages_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb5);
    let already_known = PropagationEntryRecord {
        transient_id: "b5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), already_known.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": false,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![already_known.transient_id]
    );
}

#[test]
fn peer_sync_matches_wanted_ids_by_canonical_transient_id() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa1);
    let wanted = PropagationEntryRecord {
        transient_id: "a1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&wanted).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), wanted.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [format!("  {}  ", wanted.transient_id.to_ascii_uppercase())],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(wanted.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
}

#[test]
fn peer_sync_handles_unwanted_stamped_entries_without_transfer() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa2);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_608,
        size_bytes: 24,
        stamp_value: Some(1),
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), already_known.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["messages"].as_array().expect("messages").len(), 0);
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(already_known.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![already_known.transient_id]
    );
}

#[test]
fn peer_sync_unwanted_offer_response_does_not_revive_unheard_peer_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa9);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = false;
        record.last_seen = 0;
        record.sync_backoff = 0;
        record.next_sync_attempt = 0;
        record.acceptance_rate = 0.75;
        record.sync_transfer_rate = 12_345.0;
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            already_known.transient_id.as_str(),
        )
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(result["alive"].as_bool(), Some(false));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(12_345.0));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let row = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result")["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .cloned()
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(12_345.0));
}

#[test]
fn peer_sync_transfer_limits_unwanted_oversized_entries_before_offer_response() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xa4);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = Some(80);
        record.propagation_sync_limit = Some(1_000);
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(100),
        received_at: 1_700_000_609,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), already_known.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer-limited ids"),
        &[json!(already_known.transient_id.as_str())]
    );
    assert!(
        result["propagation"]["handled_ids"]
            .as_array()
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![already_known.transient_id]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_keeps_unwanted_sync_limited_entries_queued() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-unwanted-sync-limit" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unwanted-sync-limit").expect("peer record");
        peer.propagation_sync_limit = Some(24);
    }
    let already_known = PropagationEntryRecord {
        transient_id: "a5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(100),
        received_at: 1_700_000_610,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&already_known).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unwanted-sync-limit", already_known.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-unwanted-sync-limit",
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["remaining_bytes"].as_u64(), Some(100));
    assert!(
        result["propagation"]["handled_ids"]
            .as_array()
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(already_known.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-unwanted-sync-limit")
            .expect("handled ids")
            .is_empty()
    );
    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-unwanted-sync-limit")
        .expect("pending propagation");
    assert_eq!(pending, vec![already_known]);
}

#[test]
fn peer_sync_keeps_sync_limited_entries_queued_when_peer_wants_none() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb1);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(80);
    }
    let offered = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(10),
        received_at: 1_700_000_611,
        size_bytes: 10,
        stamp_value: None,
    };
    let skipped = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(40),
        received_at: 1_700_000_612,
        size_bytes: 40,
        stamp_value: None,
    };
    for entry in [&offered, &skipped] {
        daemon.store.upsert_propagation_entry(entry).expect("store entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(offered.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(skipped.transient_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![offered.transient_id]
    );
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![skipped]);
}

#[test]
fn peer_sync_ignores_stamp_policy_for_entries_skipped_by_cumulative_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x48);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(80);
    }
    let transferable = PropagationEntryRecord {
        transient_id: "a6".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(10),
        received_at: 1_700_000_611,
        size_bytes: 10,
        stamp_value: None,
    };
    let skipped = PropagationEntryRecord {
        transient_id: "a7".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(40),
        received_at: 1_700_000_612,
        size_bytes: 40,
        stamp_value: Some(1),
    };
    for entry in [&transferable, &skipped] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(transferable.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(skipped.transient_id.as_str())]
    );

    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![transferable.transient_id]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation"),
        vec![skipped]
    );
}

#[test]
fn peer_sync_rejects_malformed_wanted_ids_without_mutating_queue() {
    let daemon = RpcDaemon::test_instance();
    let pending = PropagationEntryRecord {
        transient_id: "a3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-invalid-wanted", pending.transient_id.as_str())
        .expect("mark unhandled");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-invalid-wanted",
                "wanted_ids": ["not-a-transient-id"],
            }),
        ))
        .expect_err("malformed wanted ids should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-invalid-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-invalid-wanted")
            .expect("pending propagation"),
        vec![pending]
    );
}

#[test]
fn peer_sync_rejects_unknown_wanted_ids_without_mutating_queue() {
    let daemon = RpcDaemon::test_instance();
    let pending = PropagationEntryRecord {
        transient_id: "a8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unknown-wanted", pending.transient_id.as_str())
        .expect("mark unhandled");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-unknown-wanted",
                "wanted_ids": ["ff".repeat(32)],
            }),
        ))
        .expect_err("unknown wanted ids should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("current peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-unknown-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-unknown-wanted")
            .expect("pending propagation"),
        vec![pending]
    );
}

#[test]
fn peer_sync_rejects_unknown_wanted_ids_without_creating_new_peer_queue() {
    let daemon = RpcDaemon::test_instance();
    let existing = PropagationEntryRecord {
        transient_id: "aa".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&existing).expect("store propagation entry");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-new-unknown-wanted",
                "wanted_ids": ["ff".repeat(32)],
            }),
        ))
        .expect_err("unknown wanted ids should be rejected before peer queue creation");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("current peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-new-unknown-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-new-unknown-wanted")
            .expect("pending propagation")
            .is_empty(),
        "rejected wanted IDs must not queue existing propagation for a new peer"
    );
    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-new-unknown-wanted")),
        "rejected wanted IDs must not create a peer record"
    );
}

#[test]
fn peer_sync_rejects_transfer_limited_wanted_ids_without_mutating_queue() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-limited-wanted" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-limited-wanted").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
    }
    let pending = PropagationEntryRecord {
        transient_id: "a9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(100),
        received_at: 1_700_000_608,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-limited-wanted", pending.transient_id.as_str())
        .expect("mark unhandled");

    let error = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-limited-wanted",
                "wanted_ids": [pending.transient_id.as_str()],
            }),
        ))
        .expect_err("transfer-limited wanted id should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("current peer offer"),
        "unexpected error: {error}"
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-limited-wanted")
            .expect("handled ids")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-limited-wanted")
            .expect("pending propagation"),
        vec![pending]
    );
}

#[test]
fn list_peers_top_level_message_counters_match_python_sync_accounting() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xaf);
    let wanted = PropagationEntryRecord {
        transient_id: "af".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_609,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "b0".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(30),
        received_at: 1_700_000_610,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");

    assert_eq!(row["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["offered"].as_u64(), Some(2));
    assert_eq!(row["outgoing"].as_u64(), Some(1));
    assert_eq!(row["incoming"].as_u64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));
}

#[test]
fn peer_sync_result_reports_cumulative_acceptance_rate_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x43);
    let first = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_611,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&first).expect("store first entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), first.transient_id.as_str())
        .expect("mark first unhandled");
    daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let wanted = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_612,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            58,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("second peer sync")
        .result
        .expect("second peer sync result");
    assert_eq!(result["messages"]["offered"].as_u64(), Some(3));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(2));
    assert!(
        result["acceptance_rate"]
            .as_f64()
            .is_some_and(|value| (value - (2.0 / 3.0)).abs() < f64::EPSILON)
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(3));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(2));
    assert!(
        event.payload["acceptance_rate"]
            .as_f64()
            .is_some_and(|value| (value - (2.0 / 3.0)).abs() < f64::EPSILON)
    );
}

#[test]
fn peer_sync_persists_counters_after_propagation_entries_are_purged_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xb4);
    let wanted = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "15".repeat(24),
        received_at: 1_700_000_614,
        size_bytes: 24,
        stamp_value: None,
    };
    let already_known = PropagationEntryRecord {
        transient_id: "b5".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "16".repeat(24),
        received_at: 1_700_000_615,
        size_bytes: 24,
        stamp_value: None,
    };
    for entry in [&wanted, &already_known] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(
                peer.as_str(),
                entry.transient_id.as_str(),
            )
            .expect("mark unhandled");
    }

    let synced = daemon
        .handle_rpc(rpc_request(
            59,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(synced["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(synced["messages"]["outgoing"].as_u64(), Some(1));

    let purged = daemon
        .store
        .purge_propagation_entries_for_destination(
            wanted.destination.as_str(),
            &[wanted.transient_id.clone(), already_known.transient_id.clone()],
        )
        .expect("purge propagation entries");
    assert_eq!(purged, 2);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 60, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(row["offered"].as_u64(), Some(2));
    assert_eq!(row["outgoing"].as_u64(), Some(1));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));
}

#[test]
fn peer_sync_drops_stale_unhandled_propagation_marks() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-stale-propagation" })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-stale-propagation", "fa".repeat(32).as_str())
        .expect("mark stale unhandled");

    let before = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let before_row = before["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-stale-propagation"))
        .expect("peer row");
    assert_eq!(before_row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        before_row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        before_row["unhandled_ids"].as_array().expect("top-level unhandled ids"),
        &[] as &[JsonValue]
    );

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": "peer-stale-propagation" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));

    let after = daemon
        .handle_rpc(RpcRequest { id: 58, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let after_row = after["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-stale-propagation"))
        .expect("peer row");
    assert_eq!(after_row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(after_row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(
        after_row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        after_row["unhandled_ids"].as_array().expect("top-level unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn list_peers_ignores_stale_handled_propagation_marks() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-stale-handled" })))
        .expect("initial peer sync");
    daemon
        .store
        .mark_peer_handled_propagation("peer-stale-handled", "fb".repeat(32).as_str())
        .expect("mark stale handled");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-stale-handled"))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(row["messages"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        row["handled_ids"].as_array().expect("top-level handled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_sync_applies_per_peer_propagation_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x44);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some((24 + 20 + 32 + 16 + 1) as u32);
    }

    let small = PropagationEntryRecord {
        transient_id: "b1".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let large = PropagationEntryRecord {
        transient_id: "b2".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(100),
        received_at: 1_700_000_609,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&small).expect("store small entry");
    daemon.store.upsert_propagation_entry(&large).expect("store large entry");
    for entry in [&small, &large] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({
                "peer": peer,
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("budgeted peer sync");

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![small.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![large]);
}

#[test]
fn peer_sync_skips_entry_at_exact_sync_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-sync-equal-budget" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-sync-equal-budget").expect("peer record");
        peer.propagation_sync_limit = Some((24 + 20 + 16) as u32);
    }

    let entry = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(20),
        received_at: 1_700_000_609,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-sync-equal-budget", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": "peer-sync-equal-budget" })))
        .expect("budgeted peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(entry.transient_id.as_str())]
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-sync-equal-budget")
            .expect("handled ids")
            .is_empty()
    );
    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-sync-equal-budget")
        .expect("pending propagation");
    assert_eq!(pending, vec![entry]);
}

#[test]
fn peer_sync_applies_python_per_message_overhead_for_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x45);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some((24 + 40 + 16 + 1) as u32);
    }

    let entry = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(40),
        received_at: 1_700_000_610,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("budgeted peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![entry.transient_id]);
}

#[test]
fn peer_sync_uses_transfer_limit_when_sync_limit_is_absent() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x46);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = Some((24 + 20 + 32 + 16 + 1) as u32);
        record.propagation_sync_limit = None;
    }

    let small = PropagationEntryRecord {
        transient_id: "c1".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(20),
        received_at: 1_700_000_610,
        size_bytes: 20,
        stamp_value: None,
    };
    let large = PropagationEntryRecord {
        transient_id: "c2".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(40),
        received_at: 1_700_000_611,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&small).expect("store small entry");
    daemon.store.upsert_propagation_entry(&large).expect("store large entry");
    for entry in [&small, &large] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            59,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("budgeted peer sync");

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![small.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![large]);
}

#[test]
fn peer_sync_marks_entries_above_transfer_limit_handled_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-transfer-oversize" })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-oversize").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.alive = false;
        peer.sync_backoff = 720;
        peer.next_sync_attempt = 1_700_000_720;
    }

    let oversized = PropagationEntryRecord {
        transient_id: "c3".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_612,
        size_bytes: 100,
        stamp_value: None,
    };
    let oversized_id = oversized.transient_id.clone();
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-transfer-oversize", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": "peer-transfer-oversize" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"].as_array().expect("transfer limited ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(result["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert!(result["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids("peer-transfer-oversize")
        .expect("handled ids");
    assert_eq!(handled, vec![oversized_id.clone()]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-transfer-oversize")
        .expect("pending propagation");
    assert!(pending.is_empty());

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(event.payload["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(
        event.payload["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("event transfer limited ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(oversized_id.as_str())]
    );
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_does_not_retry_transfer_limited_entries_when_limit_increases_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-transfer-retry" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-retry").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
    }

    let oversized = PropagationEntryRecord {
        transient_id: "c4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_613,
        size_bytes: 100,
        stamp_value: None,
    };
    let oversized_id = oversized.transient_id.clone();
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-transfer-retry", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let limited = daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": "peer-transfer-retry" })))
        .expect("limited peer sync")
        .result
        .expect("limited peer sync result");
    assert_eq!(limited["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(limited["messages"]["offered"].as_u64(), Some(0));

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-retry").expect("peer record");
        peer.propagation_transfer_limit = Some(200);
        peer.propagation_sync_limit = Some(1_000);
    }

    let retried = daemon
        .handle_rpc(rpc_request(
            62,
            "peer_sync",
            json!({
                "peer": "peer-transfer-retry",
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("retried peer sync")
        .result
        .expect("retried peer sync result");
    assert_eq!(retried["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(retried["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(retried["propagation"]["transfer_limited"].as_u64(), Some(0));
    assert_eq!(
        retried["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[] as &[JsonValue]
    );
    assert!(
        retried["propagation"]["messages"].as_array().expect("messages").is_empty()
    );
    assert_eq!(retried["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(retried["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-transfer-retry")
            .expect("handled ids"),
        vec![oversized_id]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-transfer-retry")
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_applies_request_transfer_limit_without_persisting_it() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-request-limit" })))
        .expect("initial peer sync");

    let oversized = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_613,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-request-limit", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-request-limit",
                "transfer_limit_kb": 0.08,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["sync_limit"].as_u64(), Some(80));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(80));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(80));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(80));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 62, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-request-limit"))
        .expect("peer row");
    assert_eq!(row["propagation_transfer_limit"], JsonValue::Null);
}

#[test]
fn peer_sync_accepts_string_transfer_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-string-limit" })))
        .expect("initial peer sync");

    let oversized = PropagationEntryRecord {
        transient_id: "d6".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_615,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-string-limit", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-string-limit",
                "transfer_limit_kb": "0.08",
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["transfer_limit"].as_u64(), Some(80));
}

#[test]
fn peer_sync_request_transfer_limit_does_not_loosen_peer_limit() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-strict-limit" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-strict-limit").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
    }

    let oversized = PropagationEntryRecord {
        transient_id: "d5".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_614,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store oversized entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-strict-limit", oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-strict-limit",
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
}

#[test]
fn postponed_peer_sync_reports_request_transfer_limit() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": "peer-postponed-limit" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-postponed-limit").expect("peer record");
        peer.next_sync_attempt = i64::MAX;
    }

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": "peer-postponed-limit",
                "transfer_limit_kb": 0.08,
            }),
        ))
        .expect("postponed peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(80));
    assert_eq!(result["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["sync_limit"].as_u64(), Some(80));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("postponed peer sync event");
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(80));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(80));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(80));
}

#[test]
fn peer_sync_orders_offers_by_python_weight_before_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x49);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(152);
    }

    let older_large = PropagationEntryRecord {
        transient_id: "c4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(80),
        received_at: 1_700_000_612,
        size_bytes: 80,
        stamp_value: None,
    };
    let newer_small = PropagationEntryRecord {
        transient_id: "c5".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(20),
        received_at: 1_700_000_613,
        size_bytes: 20,
        stamp_value: None,
    };
    for entry in [&older_large, &newer_small] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            63,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(newer_small.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(older_large.transient_id.as_str())]
    );

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![newer_small.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![older_large]);
}

#[test]
fn peer_sync_prioritised_destinations_reduce_offer_weight_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4a);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(152);
    }
    daemon
        .handle_rpc(rpc_request(
            63,
            "set_delivery_policy",
            json!({
                "prioritised_destinations": ["17".repeat(16)],
            }),
        ))
        .expect("set delivery policy");

    let prioritised_large = PropagationEntryRecord {
        transient_id: "c6".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(80),
        received_at: 1_700_000_614,
        size_bytes: 80,
        stamp_value: None,
    };
    let normal_small = PropagationEntryRecord {
        transient_id: "c7".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_615,
        size_bytes: 20,
        stamp_value: None,
    };
    for entry in [&prioritised_large, &normal_small] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(prioritised_large.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(normal_small.transient_id.as_str())]
    );

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![prioritised_large.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![normal_small]);
}

#[test]
fn peer_sync_reports_propagation_transfer_accounting() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4a);
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some((24 + 20 + 32 + 16 + 1) as u32);
    }

    let small = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(20),
        received_at: 1_700_000_612,
        size_bytes: 20,
        stamp_value: None,
    };
    let large = PropagationEntryRecord {
        transient_id: "d2".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(100),
        received_at: 1_700_000_613,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&small).expect("store small entry");
    daemon.store.upsert_propagation_entry(&large).expect("store large entry");
    for entry in [&small, &large] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let expected_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x17; 20]])).expect("pack sync resource").len();
    assert!(
        expected_resource_bytes > small.size_bytes as usize,
        "resource accounting should include Python LXMPeer msgpack envelope overhead"
    );
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(20));
    assert_eq!(result["propagation"]["offered_bytes"].as_u64(), Some(20));
    assert_eq!(result["propagation"]["remaining"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["remaining_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["sync_limit"].as_u64(),
        Some((24 + 20 + 32 + 16 + 1) as u64)
    );
    assert_eq!(result["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(1.0));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(small.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(large.transient_id.as_str())]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["bytes"].as_u64(), Some(20));
    assert_eq!(event.payload["propagation"]["offered_bytes"].as_u64(), Some(20));
    assert_eq!(event.payload["propagation"]["remaining"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["remaining_bytes"].as_u64(), Some(100));
    assert_eq!(event.payload["synced"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["synced"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(event.payload["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["acceptance_rate"].as_f64(), Some(1.0));
    assert_eq!(
        event.payload["propagation"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(small.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["propagation"]["skipped_ids"].as_array().expect("event skipped ids"),
        &[json!(large.transient_id.as_str())]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 62, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(1.0));
}

#[test]
fn peer_sync_updates_transfer_rate_from_transferred_bytes() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4b);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d7".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "25".repeat(40),
        received_at: 1_700_000_619,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let expected_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x25; 40]])).expect("pack sync resource").len();
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(40));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(expected_resource_bytes as u64));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 65, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(expected_resource_bytes as u64));
}

#[test]
fn peer_sync_preserves_transfer_rate_when_no_offers_remain_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4c);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }

    let entry = PropagationEntryRecord {
        transient_id: "dc".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "30".repeat(48),
        received_at: 1_700_000_624,
        size_bytes: 48,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with transfer");
    let expected_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x30; 48]])).expect("pack sync resource").len();

    let result = daemon
        .handle_rpc(rpc_request(65, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("peer sync without offers")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(0));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(expected_resource_bytes as u64));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 66, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(expected_resource_bytes as u64));
}

#[test]
fn peer_sync_preserves_transfer_rate_when_offers_are_skipped_or_transfer_limited() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4d);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }

    let handled = PropagationEntryRecord {
        transient_id: "d8".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "26".repeat(40),
        received_at: 1_700_000_620,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), handled.transient_id.as_str())
        .expect("mark handled unhandled");
    daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with transfer");
    let first_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x26; 40]])).expect("pack sync resource").len();

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(24 + 40 + 16);
    }
    let skipped = PropagationEntryRecord {
        transient_id: "d9".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "27".repeat(40),
        received_at: 1_700_000_621,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&skipped).expect("store skipped entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), skipped.transient_id.as_str())
        .expect("mark skipped unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            65,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with skipped offer")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(first_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(first_resource_bytes as u64));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 66, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(first_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(first_resource_bytes as u64));

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = None;
        record.propagation_sync_limit = Some(1_000);
        record.next_sync_attempt = 0;
        record.sync_backoff = 0;
    }
    let second_handled = PropagationEntryRecord {
        transient_id: "da".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "28".repeat(32),
        received_at: 1_700_000_622,
        size_bytes: 32,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&second_handled).expect("store second handled entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            second_handled.transient_id.as_str(),
        )
        .expect("mark second handled unhandled");
    daemon
        .handle_rpc(rpc_request(
            67,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with second transfer");
    let second_resource_bytes = rmp_serde::to_vec(&(1.0_f64, vec![vec![0x27; 40], vec![0x28; 32]]))
        .expect("pack sync resource")
        .len();

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_transfer_limit = Some(80);
        record.propagation_sync_limit = Some(1_000);
    }
    let transfer_limited = PropagationEntryRecord {
        transient_id: "db".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "29".repeat(100),
        received_at: 1_700_000_623,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon
        .store
        .upsert_propagation_entry(&transfer_limited)
        .expect("store transfer limited entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            peer.as_str(),
            transfer_limited.transient_id.as_str(),
        )
        .expect("mark transfer limited unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            68,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync with transfer-limited offer")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(second_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(second_resource_bytes as u64));
}

#[test]
fn peer_sync_reports_transferred_propagation_messages() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([3u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d3".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "21".repeat(24),
        received_at: 1_700_000_614,
        size_bytes: 24,
        stamp_value: Some(11),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer_id.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let messages = result["propagation"]["messages"].as_array().expect("propagation messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["transient_id"].as_str(), Some(entry.transient_id.as_str()));
    assert_eq!(messages[0]["destination"].as_str(), Some(entry.destination.as_str()));
    assert_eq!(messages[0]["payload_hex"].as_str(), Some(entry.payload_hex.as_str()));
    assert_eq!(messages[0]["received_at"].as_i64(), Some(entry.received_at));
    assert_eq!(messages[0]["size_bytes"].as_u64(), Some(entry.size_bytes));
    assert_eq!(messages[0]["stamp_value"].as_u64(), Some(11));
}

#[test]
fn peer_sync_offers_low_value_stamped_entries_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([5u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d5".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: Some(5),
    };
    let accepted = PropagationEntryRecord {
        transient_id: "d6".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "24".repeat(24),
        received_at: 1_700_000_618,
        size_bytes: 24,
        stamp_value: Some(6),
    };
    for entry in [&low_value, &accepted] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer_id.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected_bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(accepted.transient_id.as_str()), json!(low_value.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(event.payload["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["rejected_ids"].as_array().expect("event rejected ids"),
        &[] as &[JsonValue]
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer_id.as_str())
        .expect("pending propagation");
    assert!(pending.is_empty());
    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer_id.as_str())
        .expect("handled propagation");
    assert_eq!(handled, vec![low_value.transient_id, accepted.transient_id]);
}

#[test]
fn peer_sync_postpones_low_value_stamped_entries_before_peering_key_gate_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": "peer-low-value-no-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-low-value-no-key").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d3".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: Some(5),
    };
    daemon.store.upsert_propagation_entry(&low_value).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-low-value-no-key", low_value.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": "peer-low-value-no-key" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("peering_key"));
    assert_eq!(result["peering_key"], JsonValue::Null);
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );

    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-low-value-no-key")
            .expect("pending propagation"),
        vec![low_value]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-low-value-no-key")
            .expect("handled propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_transfer_limits_low_value_stamped_entries_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": "peer-low-value-oversized" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-low-value-oversized").expect("peer record");
        peer.propagation_transfer_limit = Some(80);
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = 0;
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(100),
        received_at: 1_700_000_617,
        size_bytes: 100,
        stamp_value: Some(5),
    };
    daemon.store.upsert_propagation_entry(&low_value).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-low-value-oversized",
            low_value.transient_id.as_str(),
        )
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": "peer-low-value-oversized" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected_bytes"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited_bytes"].as_u64(), Some(100));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(
        result["propagation"]["transfer_limited_ids"]
            .as_array()
            .expect("transfer-limited ids"),
        &[json!(low_value.transient_id.as_str())]
    );

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-low-value-oversized")
            .expect("pending propagation")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-low-value-oversized")
            .expect("handled propagation"),
        vec![low_value.transient_id]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 65, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-low-value-oversized"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_postpones_low_value_stamped_entries_with_unconfigured_peering_cost_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": "peer-low-value-no-cost" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-low-value-no-cost").expect("peer record");
        peer.propagation_sync_limit = Some(1_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = None;
    }

    let low_value = PropagationEntryRecord {
        transient_id: "d0".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_617,
        size_bytes: 24,
        stamp_value: Some(5),
    };
    daemon.store.upsert_propagation_entry(&low_value).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-low-value-no-cost", low_value.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": "peer-low-value-no-cost" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("stamp_policy"));
    assert_eq!(result["peering_key_status"].as_str(), Some("unconfigured"));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["rejected_ids"].as_array().expect("rejected ids"),
        &[] as &[JsonValue]
    );

    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-low-value-no-cost")
            .expect("pending propagation"),
        vec![low_value]
    );
}

#[test]
fn peer_sync_result_and_event_report_message_accounting() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([4u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "22".repeat(24),
        received_at: 1_700_000_616,
        size_bytes: 24,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer_id.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(24));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[] as &[JsonValue]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(24));
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[] as &[JsonValue]
    );
}

#[test]
fn peer_sync_result_and_event_report_transfer_and_stamp_policy() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(65, "peer_sync", json!({ "peer": "peer-sync-policy" })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-sync-policy").expect("peer record");
        peer.name = Some("Policy Peer".to_string());
        peer.name_source = Some("test".to_string());
        peer.peer_type = Some("static".to_string());
        peer.first_seen = 1_700_000_111;
        peer.seen_count = 3;
        peer.propagation_transfer_limit = Some(333);
        peer.propagation_sync_limit = Some(999);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.sync_transfer_rate = 12_345.0;
        peer.peering_timebase = 1_700_000_123;
        peer.network_distance = 4;
        peer.rx_bytes = 55;
        peer.tx_bytes = 77;
    }

    let result = daemon
        .handle_rpc(rpc_request(66, "peer_sync", json!({ "peer": "peer-sync-policy" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("static"));
    assert_eq!(result["name"].as_str(), Some("Policy Peer"));
    assert_eq!(result["name_source"].as_str(), Some("test"));
    assert_eq!(result["first_seen"].as_i64(), Some(1_700_000_111));
    assert_eq!(result["seen_count"].as_u64(), Some(4));
    assert_eq!(result["state"].as_u64(), Some(0));
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["ler"].as_u64(), Some(0));
    assert_eq!(result["network_distance"].as_u64(), Some(4));
    assert_eq!(result["peering_timebase"].as_i64(), Some(1_700_000_123));
    assert_eq!(result["rx_bytes"].as_u64(), Some(55));
    assert_eq!(result["tx_bytes"].as_u64(), Some(77));
    assert_eq!(result["propagation_transfer_limit"].as_u64(), Some(333));
    assert_eq!(result["propagation_sync_limit"].as_u64(), Some(999));
    assert_eq!(result["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(result["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(result["transfer_limit"].as_u64(), Some(333));
    assert_eq!(result["sync_limit"].as_u64(), Some(999));
    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(333));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(999));
    assert_eq!(result["propagation"]["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(result["propagation"]["stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(result["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(result["stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(12_345.0));
    assert_eq!(result["str"].as_u64(), Some(12_345));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["peer_type"].as_str(), Some("static"));
    assert_eq!(event.payload["name"].as_str(), Some("Policy Peer"));
    assert_eq!(event.payload["name_source"].as_str(), Some("test"));
    assert_eq!(event.payload["first_seen"].as_i64(), Some(1_700_000_111));
    assert_eq!(event.payload["seen_count"].as_u64(), Some(4));
    assert_eq!(event.payload["state"].as_u64(), Some(0));
    assert_eq!(event.payload["sync_strategy"].as_u64(), Some(2));
    assert_eq!(event.payload["ler"].as_u64(), Some(0));
    assert_eq!(event.payload["network_distance"].as_u64(), Some(4));
    assert_eq!(event.payload["peering_timebase"].as_i64(), Some(1_700_000_123));
    assert_eq!(event.payload["rx_bytes"].as_u64(), Some(55));
    assert_eq!(event.payload["tx_bytes"].as_u64(), Some(77));
    assert_eq!(
        event.payload["propagation_transfer_limit"].as_u64(),
        Some(333)
    );
    assert_eq!(event.payload["propagation_sync_limit"].as_u64(), Some(999));
    assert_eq!(event.payload["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(
        event.payload["propagation_stamp_cost_flexibility"].as_u64(),
        Some(2)
    );
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(333));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(999));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(333));
    assert_eq!(event.payload["propagation"]["sync_limit"].as_u64(), Some(999));
    assert_eq!(event.payload["propagation"]["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(
        event.payload["propagation"]["stamp_cost_flexibility"].as_u64(),
        Some(2)
    );
    assert_eq!(event.payload["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(event.payload["stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(event.payload["sync_transfer_rate"].as_f64(), Some(12_345.0));
    assert_eq!(event.payload["str"].as_u64(), Some(12_345));
}

#[test]
fn list_peers_includes_propagation_marks_in_message_counters() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-propagation-stats" })))
        .expect("peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-propagation-stats").expect("peer record");
        peer.acceptance_rate = 0.9;
    }
    let handled = PropagationEntryRecord {
        transient_id: "ac".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(16),
        received_at: 1_700_000_606,
        size_bytes: 16,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "14".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon
        .store
        .mark_peer_handled_propagation("peer-propagation-stats", handled.transient_id.as_str())
        .expect("mark handled");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-propagation-stats", unhandled.transient_id.as_str())
        .expect("mark unhandled");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 57, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-propagation-stats"))
        .expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.0));
    assert_eq!(row["messages"]["offered_bytes"].as_u64(), Some(16));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(
        row["handled_ids"].as_array().expect("handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["unhandled_ids"].as_array().expect("unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
}

#[test]
fn list_peers_exposes_peering_key_value_when_cost_is_known() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([3u8; 16]);

    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_610,
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
        .expect("accept propagation peer announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 55, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["peering_cost"].as_u64(), Some(1));
    assert!(row["peering_key"].as_u64().is_some_and(|value| value >= 1));
}

#[test]
fn list_peers_exposes_peering_key_status_values() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let ready_peer = hex::encode([3u8; 16]);

    daemon
        .handle_rpc(rpc_request(55, "peer_sync", json!({ "peer": "peer-unconfigured-key" })))
        .expect("create unconfigured peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-unconfigured-key").expect("peer record");
        peer.peering_cost = None;
    }
    daemon
        .accept_announce_with_metadata(
            ready_peer.clone(),
            1_700_000_610,
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
        .expect("accept ready propagation peer announce");
    daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-not-ready-key" })))
        .expect("create not-ready peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-not-ready-key").expect("peer record");
        peer.peering_cost = Some(1);
    }

    let peers = daemon
        .handle_rpc(RpcRequest { id: 57, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    let unconfigured = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-unconfigured-key"))
        .expect("unconfigured peer row");
    let ready = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some(ready_peer.as_str()))
        .expect("ready peer row");
    let not_ready = rows
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-not-ready-key"))
        .expect("not-ready peer row");

    assert_eq!(unconfigured["peering_key"], JsonValue::Null);
    assert_eq!(unconfigured["peering_key_status"].as_str(), Some("unconfigured"));
    assert!(ready["peering_key"].as_u64().is_some_and(|value| value >= 1));
    assert_eq!(ready["peering_key_status"].as_str(), Some("ready"));
    assert_eq!(not_ready["peering_key"], JsonValue::Null);
    assert_eq!(not_ready["peering_key_status"].as_str(), Some("not_ready"));
}

#[test]
fn peer_sync_result_and_event_expose_peering_key_value_when_cost_is_known() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer = hex::encode([3u8; 16]);

    daemon
        .accept_announce_with_metadata(
            peer.clone(),
            1_700_000_611,
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
        .expect("accept propagation peer announce");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let peering_key = result["peering_key"].as_u64().expect("peering key");
    assert!(peering_key >= 1);
    assert_eq!(result["propagation"]["peering_key"].as_u64(), Some(peering_key));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["peering_key"].as_u64(), Some(peering_key));
    assert_eq!(event.payload["propagation"]["peering_key"].as_u64(), Some(peering_key));
}

#[test]
fn peer_sync_preserves_existing_auto_peer_type() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            52,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_220,
            Some("Peer Auto".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(4),
            Some(Some(1)),
            Some(Some(4)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept announce");

    daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": "peer-auto" })))
        .expect("peer sync");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer_type"].as_str(), Some("auto"));
}

#[test]
fn peer_sync_reports_python_status_type_alias() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            55,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-alias"],
            }),
        ))
        .expect("enable propagation");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let static_result = daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-static-alias" })))
        .expect("static peer sync")
        .result
        .expect("static peer sync result");
    assert_eq!(static_result["peer_type"].as_str(), Some("static"));
    assert_eq!(static_result["type"].as_str(), Some("static"));

    let static_event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("static peer sync event");
    assert_eq!(static_event.payload["peer_type"].as_str(), Some("static"));
    assert_eq!(static_event.payload["type"].as_str(), Some("static"));
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let manual_result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": "peer-manual-alias" })))
        .expect("manual peer sync")
        .result
        .expect("manual peer sync result");
    assert_eq!(manual_result["peer_type"].as_str(), Some("manual"));
    assert_eq!(manual_result["type"].as_str(), Some("discovered"));

    let manual_event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("manual peer sync event");
    assert_eq!(manual_event.payload["peer_type"].as_str(), Some("manual"));
    assert_eq!(manual_event.payload["type"].as_str(), Some("discovered"));
}

#[test]
fn stale_high_cost_announce_does_not_remove_newer_autopeer() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            55,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_400,
            None,
            None,
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
        .expect("accept initial announce");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_399,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(9)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept stale high-cost announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-auto"));
    assert_eq!(row["peer_type"].as_str(), Some("auto"));
}

#[test]
fn high_cost_announce_breaks_existing_manual_peer_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            57,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "remote_peering_cost_max": 5,
            }),
        ))
        .expect("enable propagation");

    daemon
        .handle_rpc(rpc_request(58, "peer_sync", json!({ "peer": "peer-manual" })))
        .expect("manual peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "ef".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_499,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-manual", entry.transient_id.as_str())
        .expect("mark manual peer propagation unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    daemon
        .accept_announce_with_metadata(
            "peer-manual".to_string(),
            1_700_000_500,
            None,
            None,
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(3),
            Some(Some(1)),
            Some(Some(9)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept high-cost announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 59, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-manual")
            .expect("manual peer propagation marks after break")
            .is_empty(),
        "breaking a manual peer should clear stale propagation queue marks"
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("manual peer removal event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-manual"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["reason"].as_str(), Some("peering_cost_policy"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
}

include!("status_snapshot_propagation_ingest.rs");

#[test]
fn propagation_remote_status_trims_remote_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            71,
            "propagation_remote_status",
            json!({
                "remote": "  remote-status-trimmed  ",
            }),
        ))
        .expect("remote status with padded remote")
        .result
        .expect("remote status result");

    assert_eq!(result["remote"].as_str(), Some("remote-status-trimmed"));
    assert_eq!(result["status"]["remote"].as_str(), Some("remote-status-trimmed"));
}

#[test]
fn propagation_remote_status_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let status_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::clone(&status_calls),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_remote_status",
            json!({
                "remote": "   ",
            }),
        ))
        .expect_err("blank remote status node should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(status_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

struct TestRemoteControlBridge {
    result: Result<JsonValue, std::io::ErrorKind>,
}

struct TransferLimitResultRemoteControlBridge {
    result: JsonValue,
    expected_sync_transfer_limit_kb: Option<f64>,
}

struct FailingTransferLimitRemoteControlBridge {
    kind: std::io::ErrorKind,
    expected_sync_transfer_limit_kb: Option<f64>,
}

struct RemoteSyncErrorBridge {
    kind: std::io::ErrorKind,
    message: &'static str,
}

struct CountingRemoteControlBridge {
    status_calls: Arc<std::sync::atomic::AtomicUsize>,
    download_calls: Arc<std::sync::atomic::AtomicUsize>,
    fetch_calls: Arc<std::sync::atomic::AtomicUsize>,
    sync_calls: Arc<std::sync::atomic::AtomicUsize>,
    unpeer_calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl RemoteControlBridge for TestRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "status": "ok",
        }))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, None);
        self.result.clone().map(|mut result| {
            result["remote"] = json!(remote);
            result["peer"] = json!(peer);
            result
        }).map_err(|kind| std::io::Error::new(kind, "remote sync failed"))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, None);
        self.result.clone().map(|mut result| {
            result["remote"] = json!(remote);
            result
        }).map_err(|kind| std::io::Error::new(kind, "remote download failed"))
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.result
            .clone()
            .map(|mut result| {
                result["remote"] = json!(remote);
                result["peer"] = json!(peer);
                result["unpeered"] = json!(true);
                result
            })
            .map_err(|kind| std::io::Error::new(kind, "remote unpeer failed"))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.result
            .clone()
            .map(|mut result| {
                result["remote"] = json!(remote);
                result
            })
            .map_err(|kind| std::io::Error::new(kind, "remote fetch failed"))
    }
}

impl RemoteControlBridge for CountingRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.status_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.sync_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "peer": peer,
            "synced": true,
        }))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.download_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "downloaded_count": 0,
            "messages": [],
        }))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.fetch_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "messages": [],
        }))
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.unpeer_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "peer": peer,
            "unpeered": true,
        }))
    }
}

impl RemoteControlBridge for RemoteSyncErrorBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "status": "ok",
        }))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, self.message))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "synced": true,
        }))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "messages": [],
        }))
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "peer": peer,
        }))
    }
}

impl RemoteControlBridge for TransferLimitResultRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_sync_transfer_limit_kb);
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        result["peer"] = json!(peer);
        Ok(result)
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        Ok(result)
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        Ok(result)
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        result["peer"] = json!(peer);
        result["unpeered"] = json!(true);
        Ok(result)
    }
}

impl RemoteControlBridge for FailingTransferLimitRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_sync_transfer_limit_kb);
        Err(std::io::Error::new(self.kind, "remote sync failed"))
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, "remote download failed"))
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, "remote fetch failed"))
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, "remote unpeer failed"))
    }
}

struct TransferLimitRemoteControlBridge;

impl RemoteControlBridge for TransferLimitRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, Some(42.5));
        Ok(json!({"synced": true}))
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, Some(42.5));
        Ok(json!({
            "downloaded_count": 0,
            "messages": [],
        }))
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"unpeered": true}))
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"messages": []}))
    }
}

#[test]
fn selected_propagation_node_updates_status_snapshot() {
    let daemon = RpcDaemon::test_instance();

    daemon
        .handle_rpc(rpc_request(
            67,
            "set_outbound_propagation_node",
            json!({
                "peer": "  peer-propagation-node  ",
            }),
        ))
        .expect("set propagation node");

    let propagation_status = daemon
        .handle_rpc(RpcRequest { id: 68, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        propagation_status["propagation"]["selected_node"].as_str(),
        Some("peer-propagation-node")
    );

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 69, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(
        daemon_status["propagation"]["selected_node"].as_str(),
        Some("peer-propagation-node")
    );

    let nodes = daemon
        .handle_rpc(RpcRequest {
            id: 72,
            method: "list_propagation_nodes".to_string(),
            params: None,
        })
        .expect("list propagation nodes")
        .result
        .expect("list propagation nodes result");
    let node = nodes["nodes"].as_array().and_then(|rows| rows.first()).expect("node row");
    assert_eq!(node["peer"].as_str(), Some("peer-propagation-node"));
    assert_eq!(node["selected"].as_bool(), Some(true));
    assert_eq!(node["capabilities"], json!(["propagation"]));

    daemon
        .handle_rpc(rpc_request(70, "set_outbound_propagation_node", json!({ "peer": " " })))
        .expect("clear propagation node");
    let cleared = daemon
        .handle_rpc(RpcRequest { id: 71, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(cleared["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn selected_propagation_node_queues_existing_entries_for_peer_sync() {
    let daemon = RpcDaemon::test_instance();
    let entry = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "34".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-queue" }),
        ))
        .expect("set propagation node")
        .result
        .expect("set propagation node result");
    assert_eq!(result["peer"].as_str(), Some("peer-selected-queue"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 74, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-selected-queue"))
        .expect("selected peer row");
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-selected-queue")
            .expect("list selected peer unhandled")
            .len(),
        1
    );
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
fn propagation_remote_sync_respects_peer_backoff_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(89, "peer_sync", json!({ "peer": "peer-remote-backoff" })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-backoff").expect("peer record");
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = now_i64().saturating_add(12 * 60);
        peer.alive = false;
    }
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::clone(&sync_calls),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            90,
            "propagation_remote_sync",
            json!({
                "remote": "remote-backoff",
                "peer": "peer-remote-backoff",
            }),
        ))
        .expect("remote sync should postpone")
        .result
        .expect("remote sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(result["propagation"]["postpone_reason"].as_str(), Some("backoff"));
    assert_eq!(sync_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let status = daemon
        .handle_rpc(RpcRequest { id: 91, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0x00));
    assert_eq!(status["propagation"]["last_sync_started"], JsonValue::Null);
}

#[test]
fn propagation_remote_sync_backoff_does_not_require_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(92, "peer_sync", json!({ "peer": "peer-backoff-no-bridge" })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-backoff-no-bridge").expect("peer record");
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = now_i64().saturating_add(12 * 60);
        peer.alive = false;
    }

    let result = daemon
        .handle_rpc(rpc_request(
            93,
            "propagation_remote_sync",
            json!({
                "remote": "remote-backoff-no-bridge",
                "peer": "peer-backoff-no-bridge",
            }),
        ))
        .expect("remote sync should postpone before bridge lookup")
        .result
        .expect("remote sync result");
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postponed"].as_bool(), Some(true));
    assert_eq!(result["postpone_reason"].as_str(), Some("backoff"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 94, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0x00));
    assert_eq!(status["propagation"]["last_sync_started"], JsonValue::Null);
}

#[test]
fn propagation_remote_sync_missing_bridge_does_not_create_peer() {
    let daemon = RpcDaemon::test_instance();

    let err = daemon
        .handle_rpc(rpc_request(
            95,
            "propagation_remote_sync",
            json!({
                "remote": "remote-without-bridge",
                "peer": "peer-no-bridge",
            }),
        ))
        .expect_err("missing bridge should reject remote sync");
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert_eq!(err.to_string(), "remote control bridge unavailable");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 96, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-no-bridge")),
        "missing remote bridge should not create local peer state"
    );

    let status = daemon
        .handle_rpc(RpcRequest { id: 97, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["sync_state"].as_u64(), Some(0x00));
    assert_eq!(status["propagation"]["last_sync_started"], JsonValue::Null);
}

#[test]
fn propagation_remote_unpeer_rejects_blank_peer_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let sync_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let unpeer_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls,
        unpeer_calls: Arc::clone(&unpeer_calls),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-blank-peer",
                "peer": "   ",
            }),
        ))
        .expect_err("blank remote-unpeer peer should be rejected");
    assert!(
        rejected.to_string().contains("peer is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(unpeer_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn propagation_remote_unpeer_rejects_blank_peer_without_bridge() {
    let daemon = RpcDaemon::test_instance();

    let rejected = daemon
        .handle_rpc(rpc_request(
            87,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-blank-peer",
                "peer": "   ",
            }),
        ))
        .expect_err("blank remote-unpeer peer should be rejected before bridge lookup");
    assert!(
        rejected.to_string().contains("peer is required"),
        "unexpected rejection error: {rejected}"
    );
}

#[test]
fn propagation_remote_sync_updates_lifecycle_status() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    let result_propagation = &result["propagation"];
    assert_eq!(result_propagation["sync_state"].as_u64(), Some(0x07));
    assert_eq!(result_propagation["state_name"].as_str(), Some("completed"));
    assert_eq!(result_propagation["sync_progress"].as_f64(), Some(1.0));
    assert!(result_propagation["last_sync_started"].as_i64().is_some());
    assert!(result_propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(result_propagation["last_sync_error"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 73, method: "propagation_status".to_string(), params: None })
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
fn propagation_remote_sync_updates_peer_runtime_state() {
    let payload = b"remote-sync-peer-accounting";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let peer_id = hex::encode([3u8; 16]);
    let daemon =
        RpcDaemon::with_store(MessagesStore::in_memory().expect("store"), hex::encode([2u8; 16]));
    daemon.set_remote_control_bridge(Arc::new(TransferLimitResultRemoteControlBridge {
        expected_sync_transfer_limit_kb: Some(42.5),
        result: json!({
            "synced": true,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        }),
    }));
    daemon
        .handle_rpc(rpc_request(73, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.name = Some("Remote Sync State".to_string());
        peer.name_source = Some("test".to_string());
        peer.alive = false;
        peer.sync_backoff = 12 * 60;
        peer.next_sync_attempt = 1_700_010_000;
        peer.acceptance_rate = 0.25;
        peer.propagation_transfer_limit = Some(100_000);
        peer.propagation_sync_limit = Some(84_000);
        peer.propagation_stamp_cost = Some(8);
        peer.propagation_stamp_cost_flexibility = Some(2);
        peer.peering_cost = Some(1);
    }

    let remote_sync = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": peer_id,
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect("remote sync")
        .result
        .expect("remote sync result");
    assert_eq!(remote_sync["peer_sync"]["peer"].as_str(), Some(peer_id.as_str()));
    assert_eq!(remote_sync["peer_sync"]["remote_sync"].as_bool(), Some(true));
    assert_eq!(remote_sync["peer_sync"]["synced"].as_bool(), Some(true));
    assert_eq!(remote_sync["peer_sync"]["name"].as_str(), Some("Remote Sync State"));
    assert_eq!(remote_sync["peer_sync"]["name_source"].as_str(), Some("test"));
    assert_eq!(remote_sync["peer_sync"]["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(remote_sync["peer_sync"]["tx_bytes"].as_u64(), Some(0));
    assert_eq!(remote_sync["peer_sync"]["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(remote_sync["peer_sync"]["offered"].as_u64(), Some(0));
    assert_eq!(remote_sync["peer_sync"]["outgoing"].as_u64(), Some(0));
    assert_eq!(remote_sync["peer_sync"]["incoming"].as_u64(), Some(1));
    assert_eq!(remote_sync["peer_sync"]["sync_transfer_rate"].as_f64(), Some(0.0));
    let response_peering_key =
        remote_sync["peer_sync"]["peering_key"].as_u64().expect("response peering key");
    assert!(response_peering_key >= 1);
    assert_eq!(
        remote_sync["peer_sync"]["propagation"]["peering_key"].as_u64(),
        Some(response_peering_key)
    );
    assert_eq!(remote_sync["peer_sync"]["propagation_transfer_limit"].as_u64(), Some(100_000));
    assert_eq!(remote_sync["peer_sync"]["propagation_sync_limit"].as_u64(), Some(84_000));
    assert_eq!(remote_sync["peer_sync"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(remote_sync["peer_sync"]["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(remote_sync["peer_sync"]["propagation"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(remote_sync["peer_sync"]["propagation"]["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(remote_sync["peer_sync"]["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        remote_sync["peer_sync"]["propagation"]["rejected_ids"]
            .as_array()
            .expect("response rejected ids"),
        &[] as &[JsonValue]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 75, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer_id.as_str()))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert!(row["last_sync_attempt"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(row["tx_bytes"].as_u64(), Some(0));
    assert_eq!(row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(0.0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.25));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer_id.as_str()));
    assert_eq!(event.payload["name"].as_str(), Some("Remote Sync State"));
    assert_eq!(event.payload["name_source"].as_str(), Some("test"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert!(event.payload["timestamp"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(true));
    assert_eq!(event.payload["type"].as_str(), Some("discovered"));
    assert_eq!(event.payload["state"].as_u64(), Some(0));
    assert_eq!(event.payload["sync_strategy"].as_u64(), Some(2));
    assert_eq!(event.payload["ler"].as_u64(), Some(0));
    assert_eq!(event.payload["network_distance"].as_u64(), Some(1));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_transfer_limit"].as_u64(), Some(100_000));
    assert_eq!(event.payload["propagation_sync_limit"].as_u64(), Some(84_000));
    assert_eq!(event.payload["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(
        event.payload["propagation_stamp_cost_flexibility"].as_u64(),
        Some(2)
    );
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(event.payload["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(event.payload["stamp_cost_flexibility"].as_u64(), Some(2));
    let peering_key = event.payload["peering_key"].as_u64().expect("peering key");
    assert!(peering_key >= 1);
    assert_eq!(event.payload["propagation"]["peering_key"].as_u64(), Some(peering_key));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["propagation"]["sync_limit"].as_u64(), Some(84_000));
    assert_eq!(event.payload["propagation"]["rejected"].as_u64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["rejected_ids"]
            .as_array()
            .expect("event rejected ids"),
        &[] as &[JsonValue]
    );
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(event.payload["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(event.payload["tx_bytes"].as_u64(), Some(0));
    assert_eq!(event.payload["sync_transfer_rate"].as_f64(), Some(0.0));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["offered"].as_u64(), Some(0));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(0));
    assert_eq!(event.payload["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        event.payload["messages"]["offered_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(event.payload["propagation"]["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["synced"].as_bool(), Some(true));
    assert_eq!(
        event.payload["propagation"]["imported_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        event.payload["propagation"]["duplicate_count"].as_u64(),
        Some(0)
    );
    assert_eq!(
        event.payload["propagation"]["transferred_bytes"].as_u64(),
        Some(payload.len() as u64)
    );
}

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
    assert_eq!(relay_unhandled.len(), 1);
    assert_eq!(relay_unhandled[0].transient_id, transient_id);
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
fn duplicate_propagation_remote_sync_import_does_not_double_count_received() {
    let payload = b"duplicate-remote-sync-propagation-payload";
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

    let mut second = JsonValue::Null;
    for request_id in [73, 74] {
        let result = daemon
            .handle_rpc(rpc_request(
                request_id,
                "propagation_remote_sync",
                json!({
                    "remote": "remote-node",
                    "peer": "peer-a",
                }),
            ))
            .expect("remote sync")
            .result
            .expect("remote sync result");
        second = result;
    }
    assert_eq!(second["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(second["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["result"]["imported_ids"], json!([]));
    assert_eq!(
        second["peer_sync"]["propagation"]["duplicate_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        second["peer_sync"]["messages"]["handled_ids"]
            .as_array()
            .expect("source handled ids"),
        &[json!(transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-a")
            .expect("source handled ids"),
        vec![transient_id]
    );

    let status = daemon
        .handle_rpc(RpcRequest { id: 75, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(1)
    );
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn propagation_remote_fetch_imports_payloads_into_local_store() {
    let payload = b"remote-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-fetch-relay" })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "imported_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "destination": "23".repeat(16),
                "payload_hex": payload_hex,
                "received_at": 1_700_000_700i64,
                "stamp_value": 6,
            }],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_fetch",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect("remote fetch")
        .result
        .expect("remote fetch result");
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
            74,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("local fetch after remote import")
        .result
        .expect("local fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-fetch-relay")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);
}

#[test]
fn propagation_remote_fetch_marks_source_received_and_queues_other_peers() {
    let payload = b"remote-fetch-source-peer-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-fetch-source";
    let relay_peer = "peer-fetch-source-relay";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon
        .handle_rpc(rpc_request(73, "peer_sync", json!({ "peer": relay_peer })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({ "remote": source_peer }),
        ))
        .expect("remote fetch from source peer");

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
        .handle_rpc(RpcRequest { id: 75, method: "list_peers".to_string(), params: None })
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
fn propagation_remote_fetch_trims_remote_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 0,
            "fetched_count": 0,
            "messages": [],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_fetch",
            json!({
                "remote": "  remote-fetch-trimmed  ",
            }),
        ))
        .expect("remote fetch with padded remote")
        .result
        .expect("remote fetch result");

    assert_eq!(result["remote"].as_str(), Some("remote-fetch-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-fetch-trimmed"));
}

#[test]
fn propagation_remote_fetch_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let fetch_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::clone(&fetch_calls),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_fetch",
            json!({
                "remote": "   ",
            }),
        ))
        .expect_err("blank remote fetch node should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(fetch_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn duplicate_propagation_remote_fetch_queues_known_payload_without_double_counting() {
    let payload = b"duplicate-remote-fetch-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-fetch-known" })))
        .expect("seed relay peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_remote_fetch",
            json!({ "remote": "remote-node" }),
        ))
        .expect("initial remote fetch");
    daemon
        .store
        .clear_peer_propagation_marks("peer-fetch-known")
        .expect("clear peer marks");
    let second = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_fetch",
            json!({ "remote": "remote-node" }),
        ))
        .expect("duplicate remote fetch")
        .result
        .expect("duplicate remote fetch result");
    assert_eq!(second["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(second["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["result"]["imported_ids"], json!([]));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-fetch-known")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

    let status = daemon
        .handle_rpc(RpcRequest { id: 75, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(1)
    );
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn propagation_remote_fetch_preserves_transfer_limited_peer_queue_mark_like_python() {
    let payload = b"remote-fetch-retry-transfer-limited-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-fetch-retry-limit" })))
        .expect("seed relay peer");
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: "23".repeat(16),
            payload_hex: payload_hex.clone(),
            received_at: 1_700_000_701,
            size_bytes: payload.len() as u64,
            stamp_value: None,
        })
        .expect("seed known propagation entry");
    daemon
        .store
        .mark_peer_transfer_limited_propagation("peer-fetch-retry-limit", transient_id.as_str())
        .expect("mark transfer limited");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(73, "propagation_remote_fetch", json!({ "remote": "remote-node" })))
        .expect("remote fetch")
        .result
        .expect("remote fetch result");
    assert_eq!(result["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(result["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(result["result"]["imported_ids"], json!([]));

    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-fetch-retry-limit")
        .expect("pending relay entries");
    assert!(pending.is_empty());
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-fetch-retry-limit")
            .expect("handled relay ids"),
        vec![transient_id]
    );
}

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
fn propagation_remote_download_trims_remote_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 0,
            "messages": [],
        })),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_download",
            json!({
                "remote": "  remote-download-trimmed  ",
            }),
        ))
        .expect("remote download with padded remote")
        .result
        .expect("remote download result");

    assert_eq!(result["remote"].as_str(), Some("remote-download-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-download-trimmed"));
}

#[test]
fn propagation_remote_download_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let download_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::clone(&download_calls),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_download",
            json!({
                "remote": "   ",
            }),
        ))
        .expect_err("blank remote download node should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(download_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn duplicate_propagation_remote_download_queues_known_payload_without_double_counting() {
    let payload = b"duplicate-remote-download-propagation-payload";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-download-known" })))
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
            76,
            "propagation_remote_download",
            json!({ "remote": "remote-node" }),
        ))
        .expect("initial remote download");
    daemon
        .store
        .clear_peer_propagation_marks("peer-download-known")
        .expect("clear peer marks");
    let second = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_download",
            json!({ "remote": "remote-node" }),
        ))
        .expect("duplicate remote download")
        .result
        .expect("duplicate remote download result");
    assert_eq!(second["result"]["imported_count"].as_u64(), Some(0));
    assert_eq!(second["result"]["duplicate_count"].as_u64(), Some(1));
    assert_eq!(second["result"]["imported_ids"], json!([]));

    let relay_pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-download-known")
        .expect("relay pending");
    assert_eq!(relay_pending.len(), 1);
    assert_eq!(relay_pending[0].transient_id, transient_id);

    let status = daemon
        .handle_rpc(RpcRequest { id: 78, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(1)
    );
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[test]
fn propagation_remote_download_forwards_transfer_limit_to_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TransferLimitRemoteControlBridge));

    daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_download",
            json!({
                "remote": "remote-node",
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect("remote download with transfer limit");
}

#[test]
fn propagation_remote_sync_forwards_transfer_limit_to_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TransferLimitRemoteControlBridge));

    daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-transfer-limit",
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect("remote sync with transfer limit");
}

#[test]
fn propagation_remote_sync_uses_peer_transfer_limit_when_request_limit_absent() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TransferLimitRemoteControlBridge));
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": "peer-transfer-limit" })))
        .expect("seed peer");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-transfer-limit").expect("peer record");
        peer.propagation_transfer_limit = Some(42_500);
    }

    daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-transfer-limit",
            }),
        ))
        .expect("remote sync with peer transfer limit");
}

#[test]
fn failed_propagation_remote_download_import_updates_lifecycle_error() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "imported_count": 1,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));

    let err = daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_remote_download",
            json!({
                "remote": "remote-node",
            }),
        ))
        .expect_err("remote download import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 79, method: "propagation_status".to_string(), params: None })
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
fn failed_propagation_remote_sync_updates_lifecycle_error() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));

    let err = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let status = daemon
        .handle_rpc(RpcRequest { id: 75, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &status["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_started"].as_i64().is_some());
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("remote sync failed"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 76, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-a"))
        .expect("peer row");
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("failed remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-a"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
}

#[test]
fn failed_propagation_remote_sync_updates_peer_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-remote-sync-fail" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-sync-fail").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-fail",
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-sync-fail"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 0.5));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("failed remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-sync-fail"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["alive"].as_bool(), Some(false));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(12 * 60));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 12 * 60)
    );
    assert_eq!(event.payload["propagation"]["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["synced"].as_bool(), Some(false));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote sync failed")
    );
}

#[test]
fn throttled_propagation_remote_sync_uses_python_retry_window_without_breaking_liveness() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::WouldBlock),
    }));
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-remote-throttled" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-throttled").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.75;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-throttled",
            }),
        ))
        .expect_err("remote sync throttling should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-throttled"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 180));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.75));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("throttled remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-throttled"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 180)
    );
}

#[test]
fn denied_access_propagation_remote_sync_breaks_peering_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node denied access",
    }));
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": "peer-remote-denied" })))
        .expect("initial peer sync");
    let entry = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "23".repeat(16),
        payload_hex: "23".repeat(24),
        received_at: 1_700_000_850,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-remote-denied", entry.transient_id.as_str())
        .expect("mark peer unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-denied",
            }),
        ))
        .expect_err("denied remote sync should still return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node denied access");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 80, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        !peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .any(|row| row["peer"].as_str() == Some("peer-remote-denied")),
        "denied access should break local peering"
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-remote-denied")
            .expect("list unhandled")
            .is_empty(),
        "denied access should clear peer propagation queue marks"
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("denied access unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-denied"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["reason"].as_str(), Some("access_denied"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
}

#[test]
fn identity_required_propagation_remote_sync_preserves_peer_for_immediate_retry() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation node requires identity",
    }));
    daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-remote-needs-id" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-needs-id").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.8;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-needs-id",
            }),
        ))
        .expect_err("identity-required remote sync should still return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation node requires identity");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 83, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-needs-id"))
        .expect("peer should remain available for retry");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.8));
    assert!(row["last_sync_attempt"].as_i64().expect("last sync attempt") > 0);

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "identity-required response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("identity-required peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-needs-id"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation node requires identity")
    );
}

#[test]
fn invalid_peering_key_propagation_remote_sync_preserves_peer_without_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::PermissionDenied,
        message: "propagation peer invalid peering key",
    }));
    daemon
        .handle_rpc(rpc_request(84, "peer_sync", json!({ "peer": "peer-invalid-key" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-invalid-key").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.7;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            85,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-invalid-key",
            }),
        ))
        .expect_err("invalid peering-key remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "propagation peer invalid peering key");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 86, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-invalid-key"))
        .expect("peer should remain available for another offer");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.7));
    assert!(row["last_sync_attempt"].as_i64().expect("last sync attempt") > 0);

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "invalid peering-key response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("invalid peering-key peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-invalid-key"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer invalid peering key")
    );
}

#[test]
fn invalid_data_propagation_remote_sync_preserves_peer_without_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::InvalidInput,
        message: "propagation node rejected the request",
    }));
    daemon
        .handle_rpc(rpc_request(87, "peer_sync", json!({ "peer": "peer-invalid-data" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-invalid-data").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.6;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            88,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-invalid-data",
            }),
        ))
        .expect_err("invalid-data remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), "propagation node rejected the request");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 89, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-invalid-data"))
        .expect("peer should remain available for another offer");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.6));
    assert!(row["last_sync_attempt"].as_i64().expect("last sync attempt") > 0);

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "invalid-data response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("invalid-data peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-invalid-data"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation node rejected the request")
    );
}

#[test]
fn timeout_propagation_remote_sync_preserves_peer_without_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::TimedOut,
        message: "propagation peer timed out",
    }));
    daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-timeout" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-timeout").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            91,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-timeout",
            }),
        ))
        .expect_err("timeout remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(err.to_string(), "propagation peer timed out");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 92, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-timeout"))
        .expect("peer should remain available for another offer");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.5));
    assert!(row["last_sync_attempt"].as_i64().expect("last sync attempt") > 0);

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "timeout response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("timeout peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-timeout"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer timed out")
    );
}

#[test]
fn not_found_propagation_remote_sync_preserves_peer_without_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(RemoteSyncErrorBridge {
        kind: std::io::ErrorKind::NotFound,
        message: "propagation peer not found",
    }));
    daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": "peer-not-found" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-not-found").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.4;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            94,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-not-found",
            }),
        ))
        .expect_err("not-found remote sync should return the bridge error");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(err.to_string(), "propagation peer not found");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 95, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-not-found"))
        .expect("peer should remain available for another offer");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(0.4));
    assert!(row["last_sync_attempt"].as_i64().expect("last sync attempt") > 0);

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "not-found response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .expect("not-found peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-not-found"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("propagation peer not found")
    );
}

#[test]
fn failed_propagation_remote_sync_reports_effective_limits() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(FailingTransferLimitRemoteControlBridge {
        kind: std::io::ErrorKind::TimedOut,
        expected_sync_transfer_limit_kb: Some(42.5),
    }));
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-remote-sync-limit-fail" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-sync-limit-fail").expect("peer record");
        peer.propagation_transfer_limit = Some(100_000);
        peer.propagation_sync_limit = None;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let err = daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-limit-fail",
                "transfer_limit_kb": 42.5,
            }),
        ))
        .expect_err("remote sync failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("failed remote peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-sync-limit-fail"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["remote_sync"].as_bool(), Some(true));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["propagation_transfer_limit"].as_u64(), Some(100_000));
    assert!(event.payload["propagation_sync_limit"].is_null());
    assert_eq!(event.payload["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["sync_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["propagation"]["transfer_limit"].as_u64(), Some(42_500));
    assert_eq!(event.payload["propagation"]["sync_limit"].as_u64(), Some(42_500));
    assert_eq!(
        event.payload["propagation"]["error"].as_str(),
        Some("remote sync failed")
    );
}

#[test]
fn failed_propagation_remote_sync_import_updates_peer_backoff() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "synced": true,
            "messages": [{
                "payload_hex": "not-hex",
            }],
        })),
    }));
    daemon
        .handle_rpc(rpc_request(78, "peer_sync", json!({ "peer": "peer-remote-sync-import-fail" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-remote-sync-import-fail").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.5;
    }

    let err = daemon
        .handle_rpc(rpc_request(
            79,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-sync-import-fail",
            }),
        ))
        .expect_err("remote sync import failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("invalid remote propagation payload hex"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 80, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-remote-sync-import-fail"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(false));
    assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 12 * 60));
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 0.5));
}

#[test]
fn propagation_remote_unpeer_clears_local_peer_and_queue_state() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(76, "peer_sync", json!({ "peer": "peer-remote-unpeer" })))
        .expect("peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "e1".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(24),
        received_at: 1_700_000_801,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-remote-unpeer", entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-in".to_string(),
            source: "peer-remote-unpeer".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_802,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store inbound message");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-out".to_string(),
            source: "local".to_string(),
            destination: "peer-remote-unpeer".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_803,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound message");

    let result = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer",
            }),
        ))
        .expect("remote unpeer")
        .result
        .expect("remote unpeer result");
    assert_eq!(result["removed"].as_bool(), Some(true));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(24));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(2));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(result["offered"].as_u64(), Some(1));
    assert_eq!(result["outgoing"].as_u64(), Some(1));
    assert_eq!(result["incoming"].as_u64(), Some(1));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 78, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(Vec::len), Some(0));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-remote-unpeer")
            .expect("list unhandled")
            .is_empty()
    );
}

#[test]
fn propagation_remote_unpeer_publishes_peer_removed_event() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": "peer-remote-unpeer-event" })))
        .expect("peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-event-in".to_string(),
            source: "peer-remote-unpeer-event".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_804,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store inbound message");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-remote-unpeer-event-out".to_string(),
            source: "local".to_string(),
            destination: "peer-remote-unpeer-event".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_805,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound message");

    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer-event",
            }),
        ))
        .expect("remote unpeer");

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-remote-unpeer-event"));
    assert_eq!(event.payload["remote"].as_str(), Some("remote-node"));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(event.payload["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["incoming"].as_u64(), Some(1));
}

#[test]
fn propagation_remote_unpeer_trims_remote_before_bridge_event_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));
    daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-remote-unpeer-trim" })))
        .expect("peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_unpeer",
            json!({
                "remote": "  remote-unpeer-trimmed  ",
                "peer": "peer-remote-unpeer-trim",
            }),
        ))
        .expect("remote unpeer with padded remote")
        .result
        .expect("remote unpeer result");

    assert_eq!(result["remote"].as_str(), Some("remote-unpeer-trimmed"));
    assert_eq!(result["result"]["remote"].as_str(), Some("remote-unpeer-trimmed"));

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["remote"].as_str(), Some("remote-unpeer-trimmed"));
    assert_eq!(event.payload["result"]["remote"].as_str(), Some("remote-unpeer-trimmed"));
}

#[test]
fn propagation_remote_unpeer_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let unpeer_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::clone(&unpeer_calls),
    }));
    daemon
        .handle_rpc(rpc_request(83, "peer_sync", json!({ "peer": "peer-unpeer-blank-remote" })))
        .expect("peer sync");

    let rejected = daemon
        .handle_rpc(rpc_request(
            84,
            "propagation_remote_unpeer",
            json!({
                "remote": "   ",
                "peer": "peer-unpeer-blank-remote",
            }),
        ))
        .expect_err("blank remote-unpeer remote should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(unpeer_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 85, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .any(|row| row["peer"].as_str() == Some("peer-unpeer-blank-remote")),
        "blank remote-unpeer remote should preserve the local peer"
    );
}

#[test]
fn failed_propagation_remote_unpeer_preserves_local_peer_and_queue_state() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(79, "peer_sync", json!({ "peer": "peer-remote-unpeer-fail" })))
        .expect("peer sync");

    let entry = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "1a".repeat(20),
        received_at: 1_700_000_802,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-remote-unpeer-fail", entry.transient_id.as_str())
        .expect("mark unhandled");

    let err = daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_unpeer",
            json!({
                "remote": "remote-node",
                "peer": "peer-remote-unpeer-fail",
            }),
        ))
        .expect_err("remote unpeer failure should be returned");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(err.to_string(), "remote unpeer failed");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 81, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-remote-unpeer-fail"));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled_bytes"].as_u64(), Some(20));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-remote-unpeer-fail")
            .expect("list unhandled"),
        vec![entry]
    );
}

#[test]
fn failed_propagation_remote_sync_clears_previous_completion() {
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
                "peer": "peer-a",
            }),
        ))
        .expect("initial remote sync");

    let completed = daemon
        .handle_rpc(RpcRequest { id: 77, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert!(completed["propagation"]["last_sync_completed"].as_i64().is_some());

    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    let err = daemon
        .handle_rpc(rpc_request(
            78,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect_err("second remote sync should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let failed = daemon
        .handle_rpc(RpcRequest { id: 79, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = &failed["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert!(propagation["last_sync_completed"].is_null());
    assert_eq!(propagation["last_sync_error"].as_str(), Some("remote sync failed"));
}

#[test]
fn propagation_acknowledge_sync_completion_resets_completed_state_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));
    daemon
        .handle_rpc(rpc_request(
            80,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("remote sync");

    let acknowledged = daemon
        .handle_rpc(rpc_request(
            81,
            "propagation_acknowledge_sync_completion",
            json!({}),
        ))
        .expect("acknowledge sync")
        .result
        .expect("acknowledge result");
    let propagation = &acknowledged["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x00));
    assert_eq!(propagation["state_name"].as_str(), Some("idle"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));
    assert!(propagation["last_sync_completed"].as_i64().is_some());
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn propagation_acknowledge_sync_completion_preserves_failure_without_reset() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Err(std::io::ErrorKind::TimedOut),
    }));
    daemon
        .handle_rpc(rpc_request(
            82,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect_err("remote sync failure should be returned");

    let acknowledged = daemon
        .handle_rpc(rpc_request(
            83,
            "propagation_acknowledge_sync_completion",
            json!({}),
        ))
        .expect("acknowledge failed sync")
        .result
        .expect("acknowledge result");
    let propagation = &acknowledged["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0xfe));
    assert_eq!(propagation["state_name"].as_str(), Some("failed"));
    assert_eq!(propagation["sync_progress"].as_f64(), Some(0.0));

    let reset = daemon
        .handle_rpc(rpc_request(
            84,
            "propagation_acknowledge_sync_completion",
            json!({ "reset_state": true }),
        ))
        .expect("reset failed sync")
        .result
        .expect("reset result");
    let propagation = &reset["propagation"];
    assert_eq!(propagation["sync_state"].as_u64(), Some(0x00));
    assert_eq!(propagation["state_name"].as_str(), Some("idle"));
    assert_eq!(propagation["last_sync_error"], JsonValue::Null);
}

#[test]
fn peer_types_drive_python_style_peer_counts() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            70,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static"],
            }),
        ))
        .expect("enable propagation");

    daemon
        .handle_rpc(rpc_request(71, "peer_sync", json!({ "peer": "peer-static" })))
        .expect("sync static peer");
    daemon
        .handle_rpc(rpc_request(72, "peer_sync", json!({ "peer": "peer-manual" })))
        .expect("sync manual peer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 73, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row["peer_type"].as_str() == Some("static")));
    assert!(rows.iter().any(|row| row["peer_type"].as_str() == Some("manual")));
}

#[test]
fn list_peers_static_type_tracks_current_static_peer_config() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-old"],
            }),
        ))
        .expect("enable old static peer");
    daemon
        .handle_rpc(rpc_request(75, "peer_sync", json!({ "peer": "peer-old" })))
        .expect("sync old static peer");
    daemon
        .handle_rpc(rpc_request(
            76,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-new"],
            }),
        ))
        .expect("replace static peers");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let old = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-old"))
        .expect("old peer row");
    assert_eq!(old["peer_type"].as_str(), Some("manual"));
    assert_eq!(old["type"].as_str(), Some("discovered"));
}

#[test]
fn unpeered_peers_do_not_consume_max_peer_capacity() {
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

    let first = daemon
        .handle_rpc(rpc_request(81, "peer_sync", json!({ "peer": "peer-a" })))
        .expect("sync peer-a");
    assert!(first.error.is_none());

    let blocked = daemon.handle_rpc(rpc_request(82, "peer_sync", json!({ "peer": "peer-b" })));
    assert!(blocked.is_err(), "second peer should be rejected while capacity is full");

    let unpeered = daemon
        .handle_rpc(rpc_request(83, "peer_unpeer", json!({ "peer": "peer-a" })))
        .expect("unpeer peer-a");
    assert!(unpeered.error.is_none());

    let replacement = daemon
        .handle_rpc(rpc_request(84, "peer_sync", json!({ "peer": "peer-b" })))
        .expect("sync replacement peer-b");
    assert!(replacement.error.is_none(), "replacement peer should be admitted after unpeer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 86, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["peer"].as_str(), Some("peer-b"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 85, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));
}

#[test]
fn peer_unpeer_snapshot_count_ignores_unpeered_records() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(87, "peer_sync", json!({ "peer": "peer-active" })))
        .expect("sync active peer");
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        guard.insert(
            "peer-unpeered".to_string(),
            daemon.transient_peer_record(
                "peer-unpeered".to_string(),
                1_700_000_900,
                Vec::new(),
                None,
                None,
                Some("unpeered".to_string()),
            ),
        );
    }

    daemon
        .handle_rpc(rpc_request(88, "peer_unpeer", json!({ "peer": "peer-active" })))
        .expect("unpeer active peer");

    let status = daemon
        .handle_rpc(RpcRequest { id: 89, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(0));
}

#[test]
fn peer_sync_reactivates_persisted_unpeered_record() {
    let daemon = RpcDaemon::test_instance();
    {
        let mut guard = daemon.peers.lock().expect("peers mutex poisoned");
        guard.insert(
            "peer-rejoin".to_string(),
            daemon.transient_peer_record(
                "peer-rejoin".to_string(),
                1_700_000_901,
                Vec::new(),
                None,
                None,
                Some("unpeered".to_string()),
            ),
        );
    }

    let result = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-rejoin" })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["peer_type"].as_str(), Some("manual"));

    let status = daemon
        .handle_rpc(RpcRequest { id: 91, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["peer_count"].as_u64(), Some(1));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 92, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-rejoin"));
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
}

#[test]
fn peer_unpeer_clears_persisted_propagation_queue_marks() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x52);
    daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("sync peer");
    let entry = PropagationEntryRecord {
        transient_id: "ee".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "19".repeat(24),
        received_at: 1_700_000_920,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), "fa".repeat(32).as_str())
        .expect("mark stale unhandled");

    let unpeer = daemon
        .handle_rpc(rpc_request(91, "peer_unpeer", json!({ "peer": peer.as_str() })))
        .expect("unpeer peer")
        .result
        .expect("unpeer result");
    assert_eq!(unpeer["removed"].as_bool(), Some(true));
    assert_eq!(unpeer["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(unpeer["propagation_cleared_bytes"].as_u64(), Some(24));
    assert_eq!(unpeer["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(unpeer["messages"]["unhandled"].as_u64(), Some(1));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("list unhandled")
            .is_empty()
    );

    make_ready_propagation_peer(&daemon, 0x52);
    daemon
        .handle_rpc(rpc_request(
            92,
            "peer_sync",
            json!({ "peer": peer.as_str(), "transfer_limit_kb": 1 }),
        ))
        .expect("resync peer");
    let peers = daemon
        .handle_rpc(RpcRequest { id: 93, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        row["messages"]["handled_ids"].as_array().expect("message handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn clear_peers_clears_persisted_propagation_queue_marks() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "peer-clear-queue" })))
        .expect("sync peer");
    let entry = PropagationEntryRecord {
        transient_id: "ca".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(12),
        received_at: 1_700_000_706,
        size_bytes: 12,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-clear-queue", entry.transient_id.as_str())
        .expect("mark unhandled");

    daemon
        .handle_rpc(RpcRequest { id: 91, method: "clear_peers".to_string(), params: None })
        .expect("clear peers");

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-clear-queue")
            .expect("list unhandled")
            .is_empty()
    );
}

#[test]
fn clear_peers_clears_selected_propagation_node() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            92,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-clear" }),
        ))
        .expect("select propagation node");

    daemon
        .handle_rpc(RpcRequest { id: 93, method: "clear_peers".to_string(), params: None })
        .expect("clear peers");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 95, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 96, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(daemon_status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn clear_all_clears_selected_propagation_node() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            92,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-clear-all" }),
        ))
        .expect("select propagation node");

    daemon
        .handle_rpc(RpcRequest { id: 93, method: "clear_all".to_string(), params: None })
        .expect("clear all");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 95, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn peer_unpeer_clears_selected_propagation_node() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            92,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-unpeer" }),
        ))
        .expect("select propagation node");

    daemon
        .handle_rpc(rpc_request(93, "peer_unpeer", json!({ "peer": "peer-selected-unpeer" })))
        .expect("unpeer selected node");

    let selected = daemon
        .handle_rpc(RpcRequest {
            id: 94,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected propagation node result");
    assert_eq!(selected["peer"], JsonValue::Null);

    let status = daemon
        .handle_rpc(RpcRequest { id: 95, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["selected_node"], JsonValue::Null);

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 96, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(daemon_status["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn peer_unpeer_reports_cleared_propagation_queue_accounting() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": "peer-unpeer-accounting" })))
        .expect("sync peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let handled = PropagationEntryRecord {
        transient_id: "c8".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(12),
        received_at: 1_700_000_701,
        size_bytes: 12,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "c9".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(24),
        received_at: 1_700_000_702,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon
        .store
        .mark_peer_handled_propagation("peer-unpeer-accounting", handled.transient_id.as_str())
        .expect("mark handled");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unpeer-accounting", unhandled.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-unpeer-accounting-in".to_string(),
            source: "peer-unpeer-accounting".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_703,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store inbound message");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-unpeer-accounting-out".to_string(),
            source: "local".to_string(),
            destination: "peer-unpeer-accounting".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_704,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound message");

    let result = daemon
        .handle_rpc(rpc_request(
            94,
            "peer_unpeer",
            json!({ "peer": "peer-unpeer-accounting" }),
        ))
        .expect("unpeer")
        .result
        .expect("unpeer result");
    assert_eq!(result["peer"].as_str(), Some("peer-unpeer-accounting"));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(2));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(36));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(2));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(12));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(result["offered"].as_u64(), Some(2));
    assert_eq!(result["outgoing"].as_u64(), Some(1));
    assert_eq!(result["incoming"].as_u64(), Some(1));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(2));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(36));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(2));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(12));
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(event.payload["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["incoming"].as_u64(), Some(1));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_rejects_blank_peer_identifier() {
    let daemon = RpcDaemon::test_instance();

    let err = daemon
        .handle_rpc(rpc_request(94, "peer_sync", json!({ "peer": "   " })))
        .expect_err("blank peer id should be rejected");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("peer is required"));
}

#[test]
fn lxmf_metadata_entries_merge_without_changing_receipt_status() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "metadata-message".to_string(),
            source: "source".to_string(),
            destination: "destination".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "app": "value",
                "_lxmf": {
                    "existing": true,
                },
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("insert message");

    daemon
        .record_message_lxmf_metadata_entries(
            "metadata-message",
            [
                ("propagation_packed".to_string(), json!(true)),
                ("propagation_packed_size".to_string(), json!(1234)),
                ("propagation_stamp_value".to_string(), json!(19)),
            ],
        )
        .expect("record metadata");

    let result = daemon
        .handle_rpc(RpcRequest { id: 91, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("list messages result");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some("metadata-message"))
        .expect("metadata message");

    assert_eq!(message["receipt_status"].as_str(), Some("sending"));
    assert_eq!(message["fields"]["app"].as_str(), Some("value"));
    assert_eq!(message["fields"]["_lxmf"]["existing"].as_bool(), Some(true));
    assert_eq!(message["fields"]["_lxmf"]["propagation_packed"].as_bool(), Some(true));
    assert_eq!(message["fields"]["_lxmf"]["propagation_packed_size"].as_u64(), Some(1234));
    assert_eq!(message["fields"]["_lxmf"]["propagation_stamp_value"].as_u64(), Some(19));
}
