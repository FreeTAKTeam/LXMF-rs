fn response_metadata_for_rpc(daemon: &RpcDaemon, method: &str, params: JsonValue) -> JsonValue {
    let response = daemon.handle_rpc(rpc_request(1, method, params)).expect("metadata-bearing RPC");
    assert!(response.error.is_none(), "RPC rejected: {:?}", response.error);
    response.result.expect("RPC result")["meta"].clone()
}

#[test]
fn response_metadata_preserves_default_policy_on_poll_and_node_query() {
    let daemon = RpcDaemon::test_instance();
    let expected = json!({
        "enabled": false,
        "peer_announce_at_start": false,
        "peer_announce_interval_secs": null,
        "node_announce_at_start": false,
        "node_announce_interval_secs": null,
        "transfer_limit_kb": 256,
        "sync_limit_kb": 10_240,
        "stamp_cost": 16,
        "stamp_cost_flexibility": 3,
        "peering_cost": 18,
        "control_allowed": [],
    });
    for (method, params) in [
        ("sdk_poll_events_v2", json!({"cursor": null, "max": 1})),
        ("get_outbound_propagation_node", json!({})),
    ] {
        let meta = response_metadata_for_rpc(&daemon, method, params);
        assert_eq!(meta["propagation_node"], expected);
        assert_eq!(meta["contract_version"], "v2");
        assert_eq!(meta["profile"], "desktop-full");
        assert_eq!(meta["sdk_version"], SDK_VERSION);
        assert_eq!(meta["python_reference"], python_reference_meta());
        assert!(meta["rpc_endpoint"].is_null());
        assert_eq!(meta.as_object().expect("metadata object").len(), 6);
    }
}

#[test]
fn response_metadata_projects_custom_policy_and_preserves_unrelated_history() {
    let daemon = RpcDaemon::test_instance();
    let peers: Vec<String> = (1..=512).map(|id| format!("{id:032x}")).collect();
    {
        let mut state = daemon.propagation_state.lock().expect("policy");
        state.propagation_node_enabled = true;
        state.peer_announce_at_start = true;
        state.peer_announce_interval_secs = Some(42);
        state.node_announce_at_start = true;
        state.node_announce_interval_secs = Some(84);
        state.propagation_limit = 512;
        state.sync_limit = 2048;
        state.target_cost = 7;
        state.stamp_cost_flexibility = 9;
        state.peering_cost = Some(4);
        state.control_allowed = vec!["control-z".into(), "control-a".into()];
        state.static_peers = peers.clone();
        state.store_root = Some("unrelated-durable-store".repeat(256));
        state.last_sync_error = Some("unrelated-sync-detail".repeat(256));
    }
    let before = daemon.current_propagation_state();
    let expected = json!({
        "enabled": true,
        "peer_announce_at_start": true,
        "peer_announce_interval_secs": 42,
        "node_announce_at_start": true,
        "node_announce_interval_secs": 84,
        "transfer_limit_kb": 512,
        "sync_limit_kb": 2048,
        "stamp_cost": 7,
        "stamp_cost_flexibility": 9,
        "peering_cost": 4,
        "control_allowed": ["control-z", "control-a"],
    });
    for _ in 0..3 {
        for (method, params) in [
            ("sdk_poll_events_v2", json!({"cursor": null, "max": 1})),
            ("get_outbound_propagation_node", json!({})),
        ] {
            assert_eq!(
                response_metadata_for_rpc(&daemon, method, params)["propagation_node"],
                expected
            );
        }
    }
    assert_eq!(daemon.current_propagation_state(), before);
    assert_eq!(daemon.current_propagation_state().static_peers, peers);
}

#[test]
fn response_metadata_preserves_zero_peering_cost_and_observes_policy_changes() {
    let daemon = RpcDaemon::test_instance();
    {
        let mut state = daemon.propagation_state.lock().expect("policy");
        state.target_cost = 0;
        state.peering_cost = Some(0);
        state.peer_announce_interval_secs = Some(0);
        state.control_allowed = vec!["first".into()];
    }
    let first = response_metadata_for_rpc(&daemon, "get_outbound_propagation_node", json!({}));
    assert_eq!(first["propagation_node"]["stamp_cost"], 16);
    assert_eq!(first["propagation_node"]["peering_cost"], 0);
    assert_eq!(first["propagation_node"]["peer_announce_interval_secs"], 0);
    assert_eq!(first["propagation_node"]["control_allowed"], json!(["first"]));
    {
        let mut state = daemon.propagation_state.lock().expect("updated policy");
        state.target_cost = 23;
        state.peering_cost = None;
        state.peer_announce_interval_secs = None;
        state.control_allowed.clear();
    }
    let next =
        response_metadata_for_rpc(&daemon, "sdk_poll_events_v2", json!({"cursor": null, "max": 1}));
    assert_eq!(next["propagation_node"]["stamp_cost"], 23);
    assert_eq!(next["propagation_node"]["peering_cost"], 18);
    assert!(next["propagation_node"]["peer_announce_interval_secs"].is_null());
    assert_eq!(next["propagation_node"]["control_allowed"], json!([]));
}
