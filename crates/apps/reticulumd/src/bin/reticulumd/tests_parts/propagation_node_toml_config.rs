#[test]
fn daemon_toml_parses_propagation_node_startup_config() {
    let config = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
display_name = "Field Hub"

[propagation_node]
enabled = true
control_allowed = ["00112233445566778899aabbccddeeff"]
peer_announce_at_start = true
peer_announce_interval_secs = 120
node_announce_at_start = true
node_announce_interval_secs = 300
transfer_limit_kb = 512
sync_limit_kb = 20480
stamp_cost = 21
stamp_cost_flexibility = 4
peering_cost = 23
"#,
    )
    .expect("parse propagation node config");

    let propagation = config.propagation_node.expect("propagation node config");
    assert_eq!(propagation.enabled, Some(true));
    assert_eq!(
        propagation.control_allowed,
        vec!["00112233445566778899aabbccddeeff".to_string()]
    );
    assert_eq!(propagation.peer_announce_at_start, Some(true));
    assert_eq!(propagation.peer_announce_interval_secs, Some(120));
    assert_eq!(propagation.node_announce_at_start, Some(true));
    assert_eq!(propagation.node_announce_interval_secs, Some(300));
    assert_eq!(propagation.transfer_limit_kb, Some(512));
    assert_eq!(propagation.sync_limit_kb, Some(20480));
    assert_eq!(propagation.stamp_cost, Some(21));
    assert_eq!(propagation.stamp_cost_flexibility, Some(4));
    assert_eq!(propagation.peering_cost, Some(23));
}
