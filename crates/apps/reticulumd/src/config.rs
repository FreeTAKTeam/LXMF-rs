include!("config_parts/module_prelude.rs");

include!("config_parts/interfaceconfig.rs");

include!("config_parts/non_empty_string.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direct_propagation_node_config_for_sideband_parity() {
        let config = DaemonConfig::from_toml(
            r#"
display_name = "Rust node"

[propagation_node]
enabled = true
node_announce_at_start = true
node_announce_interval_secs = 2
peer_announce_at_start = true
peer_announce_interval_secs = 3
transfer_limit_kb = 512
sync_limit_kb = 4096
stamp_cost = 19
stamp_cost_flexibility = 4
peering_cost = 21
"#,
        )
        .expect("parse config");

        let propagation_node = config.propagation_node.expect("propagation node config");
        assert_eq!(propagation_node.enabled, Some(true));
        assert_eq!(propagation_node.node_announce_at_start, Some(true));
        assert_eq!(propagation_node.node_announce_interval_secs, Some(2));
        assert_eq!(propagation_node.peer_announce_at_start, Some(true));
        assert_eq!(propagation_node.peer_announce_interval_secs, Some(3));
        assert_eq!(propagation_node.transfer_limit_kb, Some(512));
        assert_eq!(propagation_node.sync_limit_kb, Some(4096));
        assert_eq!(propagation_node.stamp_cost, Some(19));
        assert_eq!(propagation_node.stamp_cost_flexibility, Some(4));
        assert_eq!(propagation_node.peering_cost, Some(21));
    }
}
