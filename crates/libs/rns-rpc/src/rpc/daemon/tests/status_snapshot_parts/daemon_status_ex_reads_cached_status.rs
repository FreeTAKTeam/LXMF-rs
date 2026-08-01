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
    assert_status_snapshot_fields(&result);
    assert_eq!(
        daemon.metrics_snapshot()["counters"]["daemon_status_calls_total"].as_u64(),
        Some(1),
        "daemon_status_ex should record daemon-status wait metrics"
    );

    let legacy_response = daemon
        .handle_rpc(RpcRequest { id: 14, method: "status".to_string(), params: None })
        .expect("legacy status");
    let legacy = legacy_response.result.expect("legacy status result");
    assert_status_snapshot_fields(&legacy);
    assert_eq!(legacy, result);
    assert_eq!(
        daemon.metrics_snapshot()["counters"]["daemon_status_calls_total"].as_u64(),
        Some(1),
        "legacy status should not inflate daemon_status_ex metrics"
    );

    let envelope_response = daemon
        .handle_rpc(rpc_request(
            15,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "rns.runtime.status",
                "kind": "query",
                "payload": {},
            }),
        ))
        .expect("runtime status envelope");
    let envelope = envelope_response.result.expect("runtime status envelope result");
    assert_eq!(envelope["response"]["operation_id"], json!("rns.runtime.status"));
    assert_status_snapshot_fields(&envelope["response"]["payload"]);
    assert_eq!(
        daemon.metrics_snapshot()["counters"]["daemon_status_calls_total"].as_u64(),
        Some(2),
        "typed runtime status should count as a daemon status call"
    );
}

#[test]
fn delivery_policy_convenience_mutators_match_python_router_surface() {
    let daemon = RpcDaemon::test_instance();
    let destination = "00112233445566778899aabbccddeeff";
    let uppercase_destination = "00112233445566778899AABBCCDDEEFF";

    let auth_noop = daemon
        .handle_rpc(RpcRequest {
            id: 19,
            method: "set_authentication".to_string(),
            params: None,
        })
        .expect("set authentication no-op");
    assert!(auth_noop.error.is_none());

    let auth_set = daemon
        .handle_rpc(rpc_request(20, "set_authentication", json!({ "required": true })))
        .expect("set authentication");
    assert!(auth_set.error.is_none());

    let auth_get = daemon
        .handle_rpc(rpc_request(21, "requires_authentication", json!({})))
        .expect("requires authentication");
    assert_eq!(
        auth_get.result.expect("auth get result")["auth_required"].as_bool(),
        Some(true)
    );

    for (id, method, key, value) in [
        (22, "allow", "identity_hash", destination),
        (23, "allow", "destination", uppercase_destination),
        (24, "ignore_destination", "destination_hash", destination),
        (25, "prioritise", "hash", destination),
    ] {
        let response = daemon
            .handle_rpc(rpc_request(id, method, json!({ key: value })))
            .expect("policy mutator");
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
    }

    let status = daemon
        .handle_rpc(RpcRequest { id: 26, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["delivery_policy"]["auth_required"].as_bool(), Some(true));
    assert_eq!(
        status["delivery_policy"]["allowed_destinations"],
        json!([destination]),
        "allow should be idempotent and normalize hex case"
    );
    assert_eq!(status["delivery_policy"]["ignored_destinations"], json!([destination]));
    assert_eq!(status["delivery_policy"]["prioritised_destinations"], json!([destination]));

    let disallow = daemon
        .handle_rpc(rpc_request(
            27,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.delivery_policy.disallow",
                "kind": "command",
                "correlation_id": "policy-disallow-1",
                "payload": { "identity": destination },
            }),
        ))
        .expect("disallow envelope");
    assert!(disallow.error.is_none());
    let disallow_result = disallow.result.expect("disallow result");
    assert_eq!(
        disallow_result["response"]["operation_id"],
        json!("app.propagation.delivery_policy.disallow")
    );
    assert_eq!(
        disallow_result["response"]["correlation_id"],
        json!("policy-disallow-1")
    );
    assert_eq!(
        disallow_result["response"]["payload"]["policy"]["allowed_destinations"],
        json!([])
    );

    for (id, method) in [(28, "unignore_destination"), (29, "unprioritise")] {
        let response = daemon
            .handle_rpc(rpc_request(id, method, json!({ "destination": destination })))
            .expect("policy removal");
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
    }

    let final_status = daemon
        .handle_rpc(RpcRequest { id: 30, method: "daemon_status_ex".to_string(), params: None })
        .expect("final daemon status")
        .result
        .expect("final daemon status result");
    assert_eq!(final_status["delivery_policy"]["allowed_destinations"], json!([]));
    assert_eq!(final_status["delivery_policy"]["ignored_destinations"], json!([]));
    assert_eq!(final_status["delivery_policy"]["prioritised_destinations"], json!([]));

    let invalid = daemon
        .handle_rpc(rpc_request(31, "allow", json!({ "destination": "short" })))
        .expect_err("invalid destination hash should fail");
    assert_eq!(invalid.kind(), std::io::ErrorKind::InvalidInput);
}

fn assert_status_snapshot_fields(result: &JsonValue) {
    assert_eq!(result["identity_hash"].as_str(), Some("test-identity"));
    assert_eq!(result["delivery_destination_hash"].as_str(), Some("test-identity"));
    assert_eq!(result["running"].as_bool(), Some(true));
    assert_eq!(result["peer_count"].as_u64(), Some(1));
    assert_eq!(result["message_count"].as_u64(), Some(0));
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
    assert_eq!(result["reticulum"]["shared_instance"]["mode"].as_str(), Some("disabled"));
    assert_eq!(
        result["reticulum"]["shared_instance"]["share_instance"].as_bool(),
        Some(false)
    );
    assert_eq!(
        result["reticulum"]["shared_instance"]["is_shared_instance"].as_bool(),
        Some(false)
    );
    assert_eq!(
        result["reticulum"]["shared_instance"]["is_connected_to_shared_instance"].as_bool(),
        Some(false)
    );
    assert_eq!(
        result["reticulum"]["parity"],
        json!(crate::current_software_parity_orientation())
    );
    assert_eq!(
        result["reticulum"]["parity"]["overall"]["complete_ratio"],
        json!({"numerator": 1695, "denominator": 1810})
    );
    assert_eq!(
        result["reticulum"]["parity"]["reticulum"]["inventory"],
        json!({
            "total": 1608,
            "complete": 1493,
            "partial": 115,
            "not_applicable": 0
        })
    );
    assert_eq!(result["reticulum"]["parity"]["lxmf"]["level"], "complete");
    assert!(
        result["capabilities"].as_array().is_some_and(|values| values.iter().any(|value| value == "daemon_status_ex")),
        "status snapshot should include capability list: {result}"
    );
}

#[test]
fn daemon_status_ex_reports_active_shared_instance_server() {
    let daemon = RpcDaemon::test_instance();
    daemon.replace_interfaces(vec![InterfaceRecord {
        kind: "local".to_string(),
        enabled: true,
        host: Some("127.0.0.1".to_string()),
        port: Some(37428),
        name: Some("shared-instance".to_string()),
        settings: Some(json!({
            "shared_instance_type": "tcp",
            "port": 37428,
            "_runtime": {
                "startup_status": "active",
                "iface": "/00112233445566778899aabbccddeeff"
            }
        })),
    }]);

    let response = daemon
        .handle_rpc(RpcRequest { id: 16, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    let shared = &response["reticulum"]["shared_instance"];

    assert_eq!(shared["mode"].as_str(), Some("server"));
    assert_eq!(shared["share_instance"].as_bool(), Some(true));
    assert_eq!(shared["is_shared_instance"].as_bool(), Some(true));
    assert_eq!(shared["is_connected_to_shared_instance"].as_bool(), Some(false));
    assert_eq!(shared["shared_instance_type"].as_str(), Some("tcp"));
    assert_eq!(shared["endpoint"].as_str(), Some("tcp://127.0.0.1:37428"));
    assert_eq!(shared["interface_name"].as_str(), Some("shared-instance"));
}

#[test]
fn daemon_status_ex_reports_attached_shared_instance_client() {
    let daemon = RpcDaemon::test_instance();
    daemon.replace_interfaces(vec![InterfaceRecord {
        kind: "local_client".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("local-client".to_string()),
        settings: Some(json!({
            "shared_instance_type": "unix",
            "socket_path": "/tmp/reticulum.sock",
            "_runtime": {
                "startup_status": "attached",
                "iface": "/ffeeddccbbaa99887766554433221100"
            }
        })),
    }]);

    let response = daemon
        .handle_rpc(RpcRequest { id: 17, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    let shared = &response["reticulum"]["shared_instance"];

    assert_eq!(shared["mode"].as_str(), Some("client"));
    assert_eq!(shared["share_instance"].as_bool(), Some(false));
    assert_eq!(shared["is_shared_instance"].as_bool(), Some(false));
    assert_eq!(shared["is_connected_to_shared_instance"].as_bool(), Some(true));
    assert_eq!(shared["shared_instance_type"].as_str(), Some("unix"));
    assert_eq!(shared["endpoint"].as_str(), Some("/tmp/reticulum.sock"));
    assert_eq!(shared["interface_name"].as_str(), Some("local-client"));
}

#[test]
fn interface_runtime_metadata_update_refreshes_daemon_status_snapshot() {
    let daemon = RpcDaemon::test_instance();
    daemon.replace_interfaces(vec![InterfaceRecord {
        kind: "i2p".to_string(),
        enabled: true,
        host: Some("127.0.0.1".to_string()),
        port: Some(7656),
        name: Some("i2p-main".to_string()),
        settings: Some(json!({
            "_runtime": {
                "iface": "/00112233445566778899aabbccddeeff",
                "startup_status": "spawned",
                "i2p": {
                    "peer_count": 1,
                    "tunnel_status": { "accept_state": "configured" }
                }
            }
        })),
    }]);

    assert!(daemon.update_interface_runtime_metadata_by_iface(
        "/00112233445566778899aabbccddeeff",
        "i2p",
        "tunnel_status",
        json!({
            "accept_state": "listening",
            "accept_reconnect_attempts": 2,
        }),
    ));
    assert!(!daemon.update_interface_runtime_metadata_by_iface(
        "/ffffffffffffffffffffffffffffffff",
        "i2p",
        "tunnel_status",
        json!({ "accept_state": "missing" }),
    ));

    let response = daemon
        .handle_rpc(RpcRequest { id: 130, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status");
    let result = response.result.expect("daemon status result");
    let i2p = &result["interfaces"][0]["settings"]["_runtime"]["i2p"];

    assert_eq!(i2p["peer_count"].as_u64(), Some(1));
    assert_eq!(i2p["tunnel_status"]["accept_state"].as_str(), Some("listening"));
    assert_eq!(i2p["tunnel_status"]["accept_reconnect_attempts"].as_u64(), Some(2));
}

#[test]
fn propagation_enable_updates_auth_required_policy() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            14,
            "propagation_enable",
            json!({
                "enabled": true,
                "auth_required": true,
            }),
        ))
        .expect("enable propagation auth policy")
        .result
        .expect("propagation enable result");

    assert_eq!(response["propagation"]["auth_required"].as_bool(), Some(true));

    let status = daemon
        .handle_rpc(RpcRequest { id: 15, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["auth_required"].as_bool(), Some(true));
}

#[test]
fn propagation_enable_updates_control_allowed_policy() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            16,
            "propagation_enable",
            json!({
                "enabled": true,
                "control_allowed": [
                    "AABBCCDDEEFF00112233445566778899",
                    "aabbccddeeff00112233445566778899",
                    "00112233445566778899AABBCCDDEEFF"
                ],
            }),
        ))
        .expect("enable propagation control policy");
    assert!(response.error.is_none());
    let result = response.result.expect("propagation enable result");
    assert_eq!(
        result["propagation"]["control_allowed"],
        json!([
            "aabbccddeeff00112233445566778899",
            "00112233445566778899aabbccddeeff"
        ])
    );

    let status = daemon
        .handle_rpc(RpcRequest { id: 17, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["control_allowed"],
        json!([
            "aabbccddeeff00112233445566778899",
            "00112233445566778899aabbccddeeff"
        ])
    );
}

#[test]
fn propagation_control_acl_mutators_are_idempotent() {
    let daemon = RpcDaemon::test_instance();

    for id in 18..=19 {
        let response = daemon
            .handle_rpc(rpc_request(
                id,
                "allow_control",
                json!({ "identity_hash": "AABBCCDDEEFF00112233445566778899" }),
            ))
            .expect("allow control");
        assert!(response.error.is_none());
    }

    let status = daemon
        .handle_rpc(RpcRequest { id: 20, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        status["propagation"]["control_allowed"],
        json!(["aabbccddeeff00112233445566778899"])
    );

    for id in 21..=22 {
        let response = daemon
            .handle_rpc(rpc_request(
                id,
                "disallow_control",
                json!({ "hash": "aabbccddeeff00112233445566778899" }),
            ))
            .expect("disallow control");
        assert!(response.error.is_none());
        assert_eq!(response.result.expect("result")["propagation"]["control_allowed"], json!([]));
    }
}

#[test]
fn propagation_control_acl_rejects_invalid_identity_hash() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            23,
            "allow_control",
            json!({ "identity_hash": "abcd" }),
        ))
        .expect_err("invalid control hash should be rejected");
    assert_eq!(response.kind(), std::io::ErrorKind::InvalidInput);
    assert!(response.to_string().contains("16-byte"));
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
                "retain_synced_on_node": true,
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
    assert_eq!(result["propagation"]["retain_synced_on_node"].as_bool(), Some(true));
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
fn propagation_enable_matches_existing_static_peer_case_insensitively_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Static-Case";
    let configured_peer = stored_peer.to_ascii_lowercase();
    let entry = PropagationEntryRecord {
        transient_id: "a8".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_102,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .handle_rpc(rpc_request(26, "peer_sync", json!({ "peer": stored_peer })))
        .expect("seed manual peer");

    daemon
        .handle_rpc(rpc_request(
            27,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": [configured_peer],
            }),
        ))
        .expect("enable static peer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 28, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["peer"].as_str(), Some(stored_peer));
    assert_eq!(rows[0]["peer_type"].as_str(), Some("static"));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("static peer queued propagation"),
        vec![entry]
    );
}

#[test]
fn propagation_enable_queues_existing_entries_under_stored_static_peer_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Static-Queue-Case";
    let configured_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(29, "peer_sync", json!({ "peer": stored_peer })))
        .expect("seed manual peer");

    let entry = PropagationEntryRecord {
        transient_id: "a9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_103,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    daemon
        .handle_rpc(rpc_request(
            30,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": [configured_peer],
            }),
        ))
        .expect("enable static peer");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 31, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["peer"].as_str(), Some(stored_peer));
    assert_eq!(rows[0]["peer_type"].as_str(), Some("static"));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation(stored_peer)
            .expect("static peer queued propagation under stored id"),
        vec![entry]
    );
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
