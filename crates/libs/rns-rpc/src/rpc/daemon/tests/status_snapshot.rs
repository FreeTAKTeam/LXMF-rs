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
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert_eq!(peers["peers"].as_array().map(|rows| rows.len()), Some(0));
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
fn high_cost_announce_does_not_remove_manual_peer() {
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
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peer"].as_str(), Some("peer-manual"));
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
}

include!("status_snapshot_propagation_ingest.rs");

struct TestRemoteControlBridge {
    result: Result<JsonValue, std::io::ErrorKind>,
}

impl RemoteControlBridge for TestRemoteControlBridge {
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
    ) -> Result<JsonValue, std::io::Error> {
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
    ) -> Result<JsonValue, std::io::Error> {
        self.result.clone().map(|mut result| {
            result["remote"] = json!(remote);
            result
        }).map_err(|kind| std::io::Error::new(kind, "remote download failed"))
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
        Ok(json!({
            "available_count": 0,
            "fetched_count": 0,
            "imported_count": 0,
        }))
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
fn propagation_remote_sync_updates_lifecycle_status() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({"synced": true})),
    }));

    daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_remote_sync",
            json!({
                "remote": "remote-node",
                "peer": "peer-a",
            }),
        ))
        .expect("remote sync");

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
fn peer_sync_rejects_blank_peer_identifier() {
    let daemon = RpcDaemon::test_instance();

    let err = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "   " })))
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
