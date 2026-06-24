use std::fs;
use std::path::Path;

pub(crate) fn apply_python_config_file(
    effective: &mut crate::EffectiveArgs,
    paths: &crate::LxmdPaths,
) -> Result<(), String> {
    effective.config_dir = Some(paths.config_dir.clone());
    effective.db = Some(paths.storage_dir.join("reticulum.db"));
    effective.identity = Some(paths.identity_file.clone());
    effective.messages_dir = Some(paths.messages_dir.clone());

    let contents = fs::read_to_string(&paths.config_file)
        .map_err(|err| format!("failed to read {}: {err}", paths.config_file.display()))?;
    let sections = parse_python_lxmd_config(&contents);
    let interfaces = parse_python_reticulum_interfaces(&contents);
    if !interfaces.is_empty() {
        super::config::write_generated_reticulumd_config(
            paths.generated_rnsconfig.as_path(),
            &interfaces,
        )?;
        effective.rnsconfig = Some(paths.generated_rnsconfig.clone());
    }

    if let Some(lxmf) = sections.get("lxmf") {
        if let Some(value) = lxmf.get("display_name").filter(|value| !value.is_empty()) {
            effective.display_name = Some(value.clone());
        }
        if let Some(value) = lxmf.get("on_inbound").filter(|value| !value.is_empty()) {
            effective.on_inbound = Some(value.clone());
        }
        if let Some(value) = lxmf
            .get("delivery_transfer_max_accepted_size")
            .and_then(|value| value.parse::<f64>().ok())
        {
            effective.python_compat.delivery_transfer_max_kb = Some(value.max(0.0));
        }
        effective.python_compat.peer_announce_at_start = lxmf
            .get("announce_at_start")
            .and_then(|value| parse_python_bool(value).ok().flatten())
            .unwrap_or(false);
        effective.python_compat.peer_announce_interval_min =
            lxmf.get("announce_interval").and_then(|value| value.parse::<u64>().ok());
    }

    if let Some(propagation) = sections.get("propagation") {
        if let Some(enabled) =
            propagation.get("enable_node").and_then(|value| parse_python_bool(value).ok().flatten())
        {
            effective.propagation_node = enabled;
        }
        effective.python_compat.auth_required = propagation
            .get("auth_required")
            .and_then(|value| parse_python_bool(value).ok().flatten())
            .unwrap_or(false);
        effective.python_compat.autopeer = propagation
            .get("autopeer")
            .and_then(|value| parse_python_bool(value).ok().flatten())
            .unwrap_or(true);
        effective.python_compat.autopeer_maxdepth =
            propagation.get("autopeer_maxdepth").and_then(|value| value.parse::<u32>().ok());
        effective.python_compat.node_name = propagation
            .get("node_name")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        effective.python_compat.prioritised_destinations = propagation
            .get("prioritise_destinations")
            .map(|value| parse_python_list(value))
            .unwrap_or_default();
        effective.python_compat.control_allowed = propagation
            .get("control_allowed")
            .map(|value| parse_python_list(value))
            .unwrap_or_default();
        effective.python_compat.static_peers = propagation
            .get("static_peers")
            .map(|value| parse_python_list(value))
            .unwrap_or_default();
        effective.python_compat.max_peers =
            propagation.get("max_peers").and_then(|value| value.parse::<u32>().ok());
        effective.python_compat.message_storage_limit_mb =
            propagation.get("message_storage_limit").and_then(|value| value.parse::<u64>().ok());
        effective.python_compat.propagation_message_max_kb = propagation
            .get("propagation_message_max_accepted_size")
            .and_then(|value| value.parse::<f64>().ok());
        effective.python_compat.propagation_sync_max_kb = propagation
            .get("propagation_sync_max_accepted_size")
            .and_then(|value| value.parse::<f64>().ok());
        effective.python_compat.propagation_stamp_cost_target = propagation
            .get("propagation_stamp_cost_target")
            .and_then(|value| value.parse::<u32>().ok());
        effective.python_compat.propagation_stamp_cost_flexibility = propagation
            .get("propagation_stamp_cost_flexibility")
            .and_then(|value| value.parse::<u32>().ok());
        effective.python_compat.peering_cost =
            propagation.get("peering_cost").and_then(|value| value.parse::<u32>().ok());
        effective.python_compat.remote_peering_cost_max =
            propagation.get("remote_peering_cost_max").and_then(|value| value.parse::<u32>().ok());
        effective.python_compat.from_static_only = propagation
            .get("from_static_only")
            .and_then(|value| parse_python_bool(value).ok().flatten())
            .unwrap_or(false);
        effective.python_compat.retain_synced_on_node = propagation
            .get("retain_synced_on_node")
            .or_else(|| propagation.get("retain_node_lxms"))
            .and_then(|value| parse_python_bool(value).ok().flatten())
            .unwrap_or(false);
        effective.python_compat.node_announce_at_start = propagation
            .get("announce_at_start")
            .and_then(|value| parse_python_bool(value).ok().flatten())
            .unwrap_or(false);
        effective.python_compat.node_announce_interval_min =
            propagation.get("announce_interval").and_then(|value| value.parse::<u64>().ok());
        if effective.python_compat.peer_announce_interval_min.is_none() {
            effective.python_compat.peer_announce_interval_min = propagation
                .get("peer_announce_interval")
                .and_then(|value| value.parse::<u64>().ok());
        }
    }

    effective.python_compat.allowed_identities =
        read_hash_list(paths.config_dir.join("allowed").as_path())?;
    effective.python_compat.ignored_destinations =
        read_hash_list(paths.config_dir.join("ignored").as_path())?;

    Ok(())
}

pub(crate) fn parse_python_lxmd_config(
    input: &str,
) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
    let mut sections =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<String, String>>::new();
    let mut current_section = String::new();

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            sections.entry(current_section.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = strip_inline_comment(value).trim().to_string();
        sections.entry(current_section.clone()).or_default().insert(key, value);
    }

    sections
}

pub(crate) fn parse_python_reticulum_interfaces(input: &str) -> Vec<crate::SingleTomlInterface> {
    #[derive(Default)]
    struct PythonIface {
        name: Option<String>,
        iface_type: Option<String>,
        enabled: Option<bool>,
        host: Option<String>,
        port: Option<u16>,
    }

    fn push_current(out: &mut Vec<crate::SingleTomlInterface>, current: Option<PythonIface>) {
        let Some(current) = current else {
            return;
        };
        let Some(raw_type) = current.iface_type.as_deref().map(|value| value.trim()) else {
            return;
        };
        let mapped_type = match raw_type.to_ascii_lowercase().as_str() {
            "tcpserverinterface" | "tcp_server" => "tcp_server",
            "tcpclientinterface" | "tcp_client" => "tcp_client",
            _ => return,
        };
        let Some(port) = current.port else {
            return;
        };
        out.push(crate::SingleTomlInterface {
            interface_type: mapped_type.to_string(),
            enabled: current.enabled.unwrap_or(true),
            name: current.name,
            host: current.host,
            port: Some(port),
        });
    }

    let mut parsed = Vec::new();
    let mut in_interfaces = false;
    let mut current: Option<PythonIface> = None;

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            if line.starts_with("[[") && line.ends_with("]]") {
                if !in_interfaces {
                    continue;
                }
                push_current(&mut parsed, current.take());
                let name = line[2..line.len() - 2].trim();
                current = Some(PythonIface {
                    name: (!name.is_empty()).then_some(name.to_string()),
                    ..PythonIface::default()
                });
                continue;
            }

            push_current(&mut parsed, current.take());
            let section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            in_interfaces = section == "interfaces";
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !in_interfaces {
            continue;
        }
        let Some(current) = current.as_mut() else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = strip_inline_comment(value).trim();
        match key.as_str() {
            "type" => current.iface_type = Some(value.to_string()),
            "enabled" => current.enabled = parse_python_bool(value).ok().flatten(),
            "target_host" | "host" => current.host = Some(value.to_string()),
            "target_port" | "listen_port" | "port" => {
                current.port = value.parse::<u16>().ok();
            }
            "listen_ip" if !value.is_empty() => {
                current.host = Some(value.to_string());
            }
            _ => {}
        }
    }

    push_current(&mut parsed, current.take());
    for iface in &mut parsed {
        if iface.interface_type == "tcp_server"
            && iface.host.as_deref().map(str::trim).is_none_or(|value| value.is_empty())
        {
            iface.host = Some("0.0.0.0".to_string());
        }
    }
    parsed
}

fn strip_inline_comment(value: &str) -> &str {
    value.split(" #").next().unwrap_or(value)
}

fn parse_python_bool(value: &str) -> Result<Option<bool>, &'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => Ok(None),
        "yes" | "true" | "1" => Ok(Some(true)),
        "no" | "false" | "0" => Ok(Some(false)),
        _ => Err("unknown boolean value"),
    }
}

fn parse_python_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn read_hash_list(path: &Path) -> Result<Vec<String>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect())
}
