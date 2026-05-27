use serde_json::json;

use super::{config, query, rpc_client};

pub(crate) fn emit_compatibility_notes(args: &crate::Args, effective: &crate::EffectiveArgs) {
    for message in compatibility_notes(args, effective) {
        eprintln!("lxmd: {message}");
    }
}

pub(crate) fn compatibility_notes(
    args: &crate::Args,
    effective: &crate::EffectiveArgs,
) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(config_path) = args.config.as_ref() {
        let message = if config::is_single_toml_config(config_path).unwrap_or(false) {
            format!(
                "--config loaded single-file TOML settings for profile '{}' and rpc '{}'",
                effective.profile, effective.rpc
            )
        } else if config::is_legacy_launcher_toml(config_path) {
            format!(
                "--config loaded launcher settings for profile '{}' and rpc '{}'",
                effective.profile, effective.rpc
            )
        } else {
            format!(
                "--config loaded Python-style lxmd directory settings for profile '{}' and rpc '{}'",
                effective.profile, effective.rpc
            )
        };
        notes.push(message);
    }
    if args.on_inbound.is_some() {
        notes.push(
            "--on-inbound will execute a local shell command for each inbound message".to_string(),
        );
    }
    if args.status || args.peers || args.sync.is_some() || args.unpeer.is_some() {
        if args.remote.as_ref().is_some_and(|remote| query::looks_like_rpc_addr(remote)) {
            notes.push("--remote is being treated as a daemon RPC address, not a Reticulum destination hash".to_string());
        } else {
            notes.push("query mode uses the local daemon RPC to originate Python-style Reticulum destination-hash control requests".to_string());
        }
    }
    if effective.service {
        notes.push(
            "--service is accepted for compatibility and currently behaves the same as foreground mode"
                .to_string(),
        );
    }
    if args.verbose {
        notes.push("--verbose is accepted for compatibility; use standard Rust logging env vars for runtime verbosity".to_string());
    }
    if !effective.python_compat.control_allowed.is_empty() {
        notes.push(
            "control_allowed is parsed from Python config and exported to the daemon control ACL"
                .to_string(),
        );
    }
    if effective.python_compat.max_peers.is_some()
        || !effective.python_compat.static_peers.is_empty()
        || effective.python_compat.from_static_only
        || effective.python_compat.message_storage_limit_mb.is_some()
        || effective.python_compat.peering_cost.is_some()
        || effective.python_compat.remote_peering_cost_max.is_some()
    {
        notes.push("some Python propagation policy fields are loaded for compatibility output, but the Rust daemon does not enforce all of them yet".to_string());
    }
    notes
}

pub(crate) fn apply_python_compat_config(
    rpc_addr: &str,
    args: &crate::EffectiveArgs,
) -> Result<(), String> {
    let compat = &args.python_compat;
    let mut delivery_params = serde_json::Map::new();
    delivery_params.insert("auth_required".to_string(), json!(compat.auth_required));
    if !compat.allowed_identities.is_empty() {
        delivery_params
            .insert("allowed_destinations".to_string(), json!(compat.allowed_identities));
    }
    if !compat.ignored_destinations.is_empty() {
        delivery_params
            .insert("ignored_destinations".to_string(), json!(compat.ignored_destinations));
    }
    if !compat.prioritised_destinations.is_empty() {
        delivery_params
            .insert("prioritised_destinations".to_string(), json!(compat.prioritised_destinations));
    }
    if !delivery_params.is_empty() {
        rpc_client::rpc_call(
            rpc_addr,
            "set_delivery_policy",
            Some(serde_json::Value::Object(delivery_params)),
        )?;
    }

    if compat.propagation_stamp_cost_target.is_some()
        || compat.propagation_stamp_cost_flexibility.is_some()
    {
        rpc_client::rpc_call(
            rpc_addr,
            "stamp_policy_set",
            Some(json!({
                "target_cost": compat.propagation_stamp_cost_target,
                "flexibility": compat.propagation_stamp_cost_flexibility,
            })),
        )?;
    }

    if args.propagation_node || compat.propagation_stamp_cost_target.is_some() {
        rpc_client::rpc_call(
            rpc_addr,
            "propagation_enable",
            Some(json!({
                "enabled": args.propagation_node,
                "store_root": args
                    .config_dir
                    .as_ref()
                    .map(|path| path.join("storage").display().to_string()),
                "target_cost": compat.propagation_stamp_cost_target,
                "message_storage_limit_mb": compat.message_storage_limit_mb,
                "autopeer": compat.autopeer,
                "autopeer_maxdepth": compat.autopeer_maxdepth,
                "static_peers": compat.static_peers,
                "max_peers": compat.max_peers,
                "from_static_only": compat.from_static_only,
                "peering_cost": compat.peering_cost,
                "remote_peering_cost_max": compat.remote_peering_cost_max,
            })),
        )?;

        for peer in &compat.static_peers {
            let _ = rpc_client::rpc_call(rpc_addr, "peer_sync", Some(json!({ "peer": peer })));
        }
    }

    Ok(())
}
