#[derive(Clone, Debug)]
pub(super) struct ResolvedPropagationNodeConfig {
    pub(super) enabled: bool,
    pub(super) allowed_control_identities: Vec<String>,
    pub(super) peer_announce_at_start: bool,
    pub(super) peer_announce_interval_secs: Option<u64>,
    pub(super) node_announce_at_start: bool,
    pub(super) node_announce_interval_secs: Option<u64>,
    pub(super) announce_config: PropagationNodeAnnounceConfig,
}

fn resolve_propagation_node_config(
    daemon_config: Option<&DaemonConfig>,
) -> ResolvedPropagationNodeConfig {
    let toml = daemon_config.and_then(|config| config.propagation_node.as_ref());
    let defaults = PropagationNodeAnnounceConfig::default();
    let allowed_from_env = parse_hex_list_env("LXMD_CONTROL_ALLOWED");
    let allowed_control_identities = if allowed_from_env.is_empty() {
        toml.map(|config| config.control_allowed.clone()).unwrap_or_default()
    } else {
        allowed_from_env
    };

    ResolvedPropagationNodeConfig {
        enabled: env_bool("LXMD_PROPAGATION_NODE")
            .unwrap_or_else(|err| {
                log::warn!("[daemon] invalid env var LXMD_PROPAGATION_NODE: {err}");
                None
            })
            .or_else(|| toml.and_then(|config| config.enabled))
            .unwrap_or(false),
        allowed_control_identities,
        peer_announce_at_start: env_bool("LXMD_PEER_ANNOUNCE_AT_START")
            .unwrap_or_else(|err| {
                log::warn!("[daemon] invalid env var LXMD_PEER_ANNOUNCE_AT_START: {err}");
                None
            })
            .or_else(|| toml.and_then(|config| config.peer_announce_at_start))
            .unwrap_or(false),
        peer_announce_interval_secs: env_u64("LXMD_PEER_ANNOUNCE_INTERVAL_SECS")
            .unwrap_or_else(|err| {
                log::warn!("[daemon] invalid env var LXMD_PEER_ANNOUNCE_INTERVAL_SECS: {err}");
                None
            })
            .or_else(|| toml.and_then(|config| config.peer_announce_interval_secs))
            .filter(|value| *value > 0),
        node_announce_at_start: env_bool("LXMD_NODE_ANNOUNCE_AT_START")
            .unwrap_or_else(|err| {
                log::warn!("[daemon] invalid env var LXMD_NODE_ANNOUNCE_AT_START: {err}");
                None
            })
            .or_else(|| toml.and_then(|config| config.node_announce_at_start))
            .unwrap_or(false),
        node_announce_interval_secs: env_u64("LXMD_NODE_ANNOUNCE_INTERVAL_SECS")
            .unwrap_or_else(|err| {
                log::warn!("[daemon] invalid env var LXMD_NODE_ANNOUNCE_INTERVAL_SECS: {err}");
                None
            })
            .or_else(|| toml.and_then(|config| config.node_announce_interval_secs))
            .filter(|value| *value > 0),
        announce_config: PropagationNodeAnnounceConfig {
            transfer_limit_kb: toml
                .and_then(|config| config.transfer_limit_kb)
                .filter(|value| *value > 0)
                .unwrap_or(defaults.transfer_limit_kb),
            sync_limit_kb: toml
                .and_then(|config| config.sync_limit_kb)
                .filter(|value| *value > 0)
                .unwrap_or(defaults.sync_limit_kb),
            stamp_cost: toml
                .and_then(|config| config.stamp_cost)
                .filter(|value| *value > 0)
                .unwrap_or(defaults.stamp_cost),
            stamp_cost_flexibility: toml
                .and_then(|config| config.stamp_cost_flexibility)
                .unwrap_or(defaults.stamp_cost_flexibility),
            peering_cost: toml
                .and_then(|config| config.peering_cost)
                .filter(|value| *value > 0)
                .unwrap_or(defaults.peering_cost),
            ..defaults
        },
    }
}

fn env_bool(key: &str) -> Result<Option<bool>, &'static str> {
    let value = match std::env::var(key) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(Some(true)),
        "0" | "false" | "no" | "off" => Ok(Some(false)),
        _ => Err("unrecognised boolean value"),
    }
}
