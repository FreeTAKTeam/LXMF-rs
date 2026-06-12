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
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(333_000));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(999_000));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(5));
    assert_eq!(row["name"].as_str(), Some("Static PN"));
}

#[test]
fn static_peer_path_response_matches_existing_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Static-Path-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(
            56,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": [stored_peer],
            }),
        ))
        .expect("enable static peer");
    let initial_app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_024i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(222),
        MsgPackValue::from(888),
        MsgPackValue::Array(vec![
            MsgPackValue::from(7),
            MsgPackValue::from(2),
            MsgPackValue::from(4),
        ]),
        MsgPackValue::Map(vec![(
            MsgPackValue::from(1),
            MsgPackValue::Binary(b"Static Case PN".to_vec()),
        )]),
    ]))
    .expect("encode initial propagation app data");
    daemon
        .handle_rpc(rpc_request(
            57,
            "announce_received",
            json!({
                "peer": stored_peer,
                "timestamp": 1_700_000_140i64,
                "app_data_hex": hex::encode(initial_app_data),
                "aspect": "lxmf.propagation",
            }),
        ))
        .expect("initial announce");

    let path_response_app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_125i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(555),
        MsgPackValue::from(1110),
        MsgPackValue::Array(vec![
            MsgPackValue::from(10),
            MsgPackValue::from(3),
            MsgPackValue::from(6),
        ]),
        MsgPackValue::Map(vec![(
            MsgPackValue::from(1),
            MsgPackValue::Binary(b"Path Case PN".to_vec()),
        )]),
    ]))
    .expect("encode path response propagation app data");
    daemon
        .handle_rpc(rpc_request(
            58,
            "announce_received",
            json!({
                "peer": request_peer,
                "timestamp": 1_700_000_150i64,
                "app_data_hex": hex::encode(path_response_app_data),
                "aspect": "lxmf.propagation",
                "is_path_response": true,
            }),
        ))
        .expect("path response announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 59, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    let row = rows.iter().find(|row| row["peer"].as_str() == Some(stored_peer)).expect("static row");
    assert_eq!(row["last_heard"].as_i64(), Some(1_700_000_140));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_024));
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(222_000));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(888_000));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(7));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(4));
    assert_eq!(row["name"].as_str(), Some("Static Case PN"));
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
    assert_eq!(row["metadata"]["name"].as_str(), Some("PN Alpha"));
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
