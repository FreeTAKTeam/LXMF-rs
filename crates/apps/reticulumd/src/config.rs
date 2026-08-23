include!("config_parts/module_prelude.rs");

include!("config_parts/reticulum_runtime_policy.rs");

include!("config_parts/daemonconfig_impl.rs");

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

    #[test]
    fn i2p_sam_address_env_default_parser_matches_python_shape() {
        assert_eq!(parse_i2p_sam_address("127.0.0.2:8765"), Some(("127.0.0.2".to_string(), 8765)));
        assert_eq!(
            parse_i2p_sam_address(" sam.example:7656 "),
            Some(("sam.example".to_string(), 7656))
        );
        assert_eq!(parse_i2p_sam_address("127.0.0.1"), None);
        assert_eq!(parse_i2p_sam_address(":7656"), None);
        assert_eq!(parse_i2p_sam_address("127.0.0.1:not-a-port"), None);
    }

    #[test]
    fn rns_1_5_queue_lengths_parse_with_python_defaults_and_positive_overrides() {
        let defaults =
            ReticulumRuntimePolicy::from_toml("[reticulum]").expect("parse default policy");
        assert_eq!(
            defaults.inbound_queue_limits(),
            rns_transport::transport::InboundQueueLimits::default()
        );

        let configured = ReticulumRuntimePolicy::from_toml(
            "[reticulum]\nqlen_in_data=10\nqlen_in_announce=11\nqlen_in_pr=12\nqlen_in_il=13",
        )
        .expect("parse configured policy");
        assert_eq!(configured.inbound_queue_limits().data, 10);
        assert_eq!(configured.inbound_queue_limits().announce, 11);
        assert_eq!(configured.inbound_queue_limits().path_request, 12);
        assert_eq!(configured.inbound_queue_limits().ingress_limited, 13);
    }
}
