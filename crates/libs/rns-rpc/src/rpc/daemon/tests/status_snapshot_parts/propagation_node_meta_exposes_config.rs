#[test]
fn propagation_node_get_meta_exposes_current_node_config() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_propagation_state(true, None, 21);

    let result = daemon
        .handle_rpc(rpc_request(77, "get_outbound_propagation_node", json!({})))
        .expect("node get")
        .result
        .expect("node get result");
    let config = &result["meta"]["propagation_node"];

    assert_eq!(config["enabled"], json!(true));
    assert_eq!(config["transfer_limit_kb"], json!(256));
    assert_eq!(config["sync_limit_kb"], json!(10240));
    assert_eq!(config["stamp_cost"], json!(21));
    assert_eq!(config["stamp_cost_flexibility"], json!(3));
    assert_eq!(config["peering_cost"], json!(18));
}
