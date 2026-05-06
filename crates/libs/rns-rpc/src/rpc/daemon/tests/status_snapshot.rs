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

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_100,
            Some("Peer Auto".to_string()),
            Some("announce".to_string()),
            None,
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
    assert_eq!(row["sync_backoff"].as_u64(), Some(1));
    assert!(row["acceptance_rate"].as_f64().is_some_and(|value| value < 1.0));
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

#[test]
fn propagation_counters_track_ingest_and_unpeered_attempts() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = [0x00_u8, 0x11_u8];
    let transient_id = hex::encode(Sha256::digest(payload));
    daemon
        .handle_rpc(rpc_request(
            60,
            "propagation_ingest",
            json!({
                "transient_id": transient_id,
                "payload_hex": hex::encode(payload),
            }),
        ))
        .expect("propagation ingest");
    daemon.record_unpeered_propagation_attempt(42);

    let result = daemon
        .handle_rpc(RpcRequest { id: 61, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    let propagation = result["propagation"].clone();
    assert_eq!(propagation["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(propagation["client_propagation_messages_served"].as_u64(), Some(0));
    assert_eq!(propagation["unpeered_propagation_incoming"].as_u64(), Some(1));
    assert_eq!(propagation["unpeered_propagation_rx_bytes"].as_u64(), Some(42));

    daemon
        .handle_rpc(rpc_request(
            62,
            "propagation_fetch",
            json!({
                "transient_id": hex::encode(Sha256::digest(payload)),
            }),
        ))
        .expect("propagation fetch");
    let result = daemon
        .handle_rpc(RpcRequest { id: 63, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(result["propagation"]["client_propagation_messages_served"].as_u64(), Some(1));
}

#[test]
fn propagation_ingest_rejects_missing_stamp_when_cost_required() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            64,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 1,
            }),
        ))
        .expect("enable propagation");

    let err = daemon
        .handle_rpc(rpc_request(
            65,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(b"unstamped-propagation-bytes"),
            }),
        ))
        .expect_err("missing propagation stamp must be rejected");
    assert!(err.to_string().contains("invalid propagation stamp"));

    let result = daemon
        .handle_rpc(RpcRequest { id: 66, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(result["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
}

#[test]
fn propagation_ingest_rejects_too_short_stamped_payload_when_cost_required() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            67,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 1,
            }),
        ))
        .expect("enable propagation");

    let too_short_transient = vec![0xAA_u8; 112 + 32];
    let err = daemon
        .handle_rpc(rpc_request(
            68,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(too_short_transient),
            }),
        ))
        .expect_err("too-short stamped propagation payload must be rejected");
    assert!(err.to_string().contains("invalid propagation stamp"));
}

#[test]
fn propagation_ingest_accepts_valid_stamp_when_cost_required() {
    use sha2::Digest;

    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            68,
            "propagation_enable",
            json!({
                "enabled": true,
                "target_cost": 1,
            }),
        ))
        .expect("enable propagation");

    let lxm_data = vec![0x42_u8; 113];
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let response = daemon
        .handle_rpc(rpc_request(
            69,
            "propagation_ingest",
            json!({
                "payload_hex": hex::encode(&transient_data),
            }),
        ))
        .expect("valid stamped propagation ingest");
    let result = response.result.expect("ingest result");
    let transient_id = result["transient_id"].as_str().expect("transient id").to_string();
    assert_eq!(transient_id, hex::encode(sha2::Sha256::digest(&lxm_data)));
    assert_eq!(result["ingested_count"].as_u64(), Some(1));

    let fetched = daemon
        .handle_rpc(rpc_request(
            69,
            "propagation_fetch",
            json!({
                "transient_id": transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    let expected_payload_hex = hex::encode(&lxm_data);
    assert_eq!(fetched["payload_hex"].as_str(), Some(expected_payload_hex.as_str()));
}

#[test]
fn propagation_ingest_derives_transient_id_from_payload_bytes() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let payload = b"unstamped-propagation-wire-payload";
    let payload_hex = hex::encode(payload);
    let expected_transient_id = hex::encode(Sha256::digest(payload));

    let response = daemon
        .handle_rpc(rpc_request(
            70,
            "propagation_ingest",
            json!({
                "payload_hex": payload_hex,
            }),
        ))
        .expect("propagation ingest");
    let result = response.result.expect("ingest result");
    assert_eq!(result["transient_id"].as_str(), Some(expected_transient_id.as_str()));

    let fetched = daemon
        .handle_rpc(rpc_request(
            71,
            "propagation_fetch",
            json!({
                "transient_id": expected_transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(payload_hex.as_str()));
}

#[test]
fn propagation_ingest_accepts_stamped_payload_with_canonical_transient_id_when_cost_is_zero() {
    use sha2::{Digest, Sha256};

    let daemon = RpcDaemon::test_instance();
    let lxm_data = vec![0x55_u8; 113];
    let transient_data = stamped_propagation_payload(&lxm_data, 1);
    let canonical_transient_id = hex::encode(Sha256::digest(&lxm_data));

    let response = daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_ingest",
            json!({
                "transient_id": canonical_transient_id,
                "payload_hex": hex::encode(&transient_data),
            }),
        ))
        .expect("stamped propagation ingest at zero target cost");
    let result = response.result.expect("ingest result");
    assert_eq!(result["ingested_count"].as_u64(), Some(1));
    assert_eq!(result["transient_id"].as_str(), Some(canonical_transient_id.as_str()));

    let fetched = daemon
        .handle_rpc(rpc_request(
            73,
            "propagation_fetch",
            json!({
                "transient_id": canonical_transient_id,
            }),
        ))
        .expect("fetch ingested propagation payload")
        .result
        .expect("fetch result");
    let expected_payload_hex = hex::encode(&lxm_data);
    assert_eq!(fetched["payload_hex"].as_str(), Some(expected_payload_hex.as_str()));
}

#[test]
fn propagation_ingest_rejects_mismatched_transient_id() {
    let daemon = RpcDaemon::test_instance();
    let err = daemon
        .handle_rpc(rpc_request(
            74,
            "propagation_ingest",
            json!({
                "transient_id": "deadbeef",
                "payload_hex": hex::encode(b"propagation-wire-payload"),
            }),
        ))
        .expect_err("mismatched transient_id must be rejected");
    assert!(err.to_string().contains("transient_id does not match propagation payload"));
}

#[test]
fn propagation_ingest_without_payload_does_not_increment_counts_or_store_payload() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            75,
            "propagation_ingest",
            json!({
                "transient_id": "abcd",
            }),
        ))
        .expect("ingest without payload");
    let result = response.result.expect("ingest result");
    assert_eq!(result["ingested_count"].as_u64(), Some(0));
    assert_eq!(result["transient_id"].as_str(), Some("abcd"));

    let snapshot = daemon
        .handle_rpc(RpcRequest { id: 76, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(snapshot["propagation"]["last_ingest_count"].as_u64(), Some(0));
    assert_eq!(snapshot["propagation"]["total_ingested"].as_u64(), Some(0));
    assert_eq!(
        snapshot["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(0)
    );

    let err = daemon
        .handle_rpc(rpc_request(
            77,
            "propagation_fetch",
            json!({
                "transient_id": "abcd",
            }),
        ))
        .expect_err("payload should not have been stored");
    assert!(err.to_string().contains("transient_id not found"));
}

fn stamped_propagation_payload(lxm_data: &[u8], target_cost: u32) -> Vec<u8> {
    use hkdf::Hkdf;
    use sha2::{Digest, Sha256};

    const PROPAGATION_STAMP_SIZE: usize = 32;
    const PROPAGATION_STAMP_ROUNDS: usize = 1000;

    let transient_id = Sha256::digest(lxm_data);
    let mut workblock = Vec::with_capacity(PROPAGATION_STAMP_ROUNDS * 256);
    for round in 0..PROPAGATION_STAMP_ROUNDS {
        let mut salt_data = Vec::with_capacity(transient_id.len() + 8);
        salt_data.extend_from_slice(transient_id.as_slice());
        let packed =
            rmp_serde::to_vec(&(round as u32)).expect("msgpack encode propagation stamp round");
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), transient_id.as_slice());
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).expect("hkdf expand propagation stamp workblock");
        workblock.extend_from_slice(&okm);
    }

    let mut stamp = vec![0u8; PROPAGATION_STAMP_SIZE];
    let mut nonce = 0u64;
    loop {
        stamp[..8].copy_from_slice(&nonce.to_le_bytes());
        let mut material = Vec::with_capacity(workblock.len() + stamp.len());
        material.extend_from_slice(&workblock);
        material.extend_from_slice(&stamp);
        let hash = Sha256::digest(&material);
        let mut value = 0u32;
        for byte in hash {
            if byte == 0 {
                value += 8;
            } else {
                value += byte.leading_zeros();
                break;
            }
        }
        if value >= target_cost {
            break;
        }
        nonce = nonce.wrapping_add(1);
    }

    let mut transient = lxm_data.to_vec();
    transient.extend_from_slice(&stamp);
    transient
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
fn peer_sync_rejects_blank_peer_identifier() {
    let daemon = RpcDaemon::test_instance();

    let err = daemon
        .handle_rpc(rpc_request(90, "peer_sync", json!({ "peer": "   " })))
        .expect_err("blank peer id should be rejected");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("peer is required"));
}
