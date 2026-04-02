use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn load_effective_args(args: &crate::Args) -> Result<crate::EffectiveArgs, String> {
    let mut effective = crate::EffectiveArgs {
        profile: "default".to_string(),
        rpc: crate::DEFAULT_RPC_ADDR.to_string(),
        rnsconfig: None,
        propagation_node: false,
        on_inbound: None,
        quiet: false,
        service: false,
        display_name: None,
        db: None,
        identity: None,
        transport: None,
        reticulumd: None,
        messages_dir: None,
        config_dir: None,
        timeout_secs: 5.0,
        status: false,
        peers: false,
        sync: None,
        unpeer: None,
        remote: None,
        query_identity: None,
        python_compat: crate::PythonCompatConfig::default(),
    };
    if let Some(config_path) = args.config.as_ref() {
        if is_single_toml_config(config_path)? {
            let paths = prepare_lxmd_paths(Some(config_path))?;
            apply_single_toml_config(&mut effective, config_path, &paths)?;
        } else if is_legacy_launcher_toml(config_path) {
            let paths = prepare_lxmd_paths(Some(config_path))?;
            apply_python_config_file(&mut effective, &paths)?;
            apply_config_file(&mut effective, config_path)?;
        } else {
            let paths = prepare_lxmd_paths(Some(config_path))?;
            apply_python_config_file(&mut effective, &paths)?;
        }
    } else {
        let paths = prepare_lxmd_paths(args.config.as_deref())?;
        apply_python_config_file(&mut effective, &paths)?;
    }

    effective.profile = args.profile.clone();
    if let Some(rpc) = args.rpc.as_ref() {
        effective.rpc = rpc.clone();
    }
    if let Some(rnsconfig) = args.rnsconfig.as_ref() {
        effective.rnsconfig = Some(rnsconfig.clone());
    }
    if args.propagation_node {
        effective.propagation_node = true;
    }
    if let Some(on_inbound) = args.on_inbound.as_ref() {
        effective.on_inbound = Some(on_inbound.clone());
    }
    if args.quiet {
        effective.quiet = true;
    }
    if args.service {
        effective.service = true;
    }
    if let Some(timeout) = args.timeout {
        effective.timeout_secs = timeout.max(0.1);
    }
    effective.status = args.status;
    effective.peers = args.peers;
    effective.sync = args.sync.clone();
    effective.unpeer = args.unpeer.clone();
    effective.remote = args.remote.clone();
    effective.query_identity = args.identity.clone();
    Ok(effective)
}

pub(crate) fn prepare_lxmd_paths(config_arg: Option<&Path>) -> Result<crate::LxmdPaths, String> {
    let config_dir = if let Some(path) = config_arg {
        if path.is_dir()
            || (!path.exists()
                && path.extension().is_none()
                && path.file_name().and_then(|name| name.to_str()) != Some("config"))
        {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
        }
    } else {
        default_lxmd_config_dir()?
    };
    let config_file = match config_arg {
        Some(path) if path.is_file() && !is_legacy_launcher_toml(path) => path.to_path_buf(),
        _ => config_dir.join("config"),
    };
    let identity_file = config_dir.join("identity");
    let storage_dir = config_dir.join("storage");
    let messages_dir = storage_dir.join("messages");
    let generated_rnsconfig = config_dir.join(crate::GENERATED_RETICULUMD_CONFIG);

    fs::create_dir_all(&messages_dir)
        .map_err(|err| format!("failed to create {}: {err}", messages_dir.display()))?;
    if !config_file.exists() {
        fs::write(&config_file, crate::PYTHON_DEFAULT_LXMD_CONFIG)
            .map_err(|err| format!("failed to create {}: {err}", config_file.display()))?;
    }

    Ok(crate::LxmdPaths {
        config_dir,
        config_file,
        identity_file,
        storage_dir,
        messages_dir,
        generated_rnsconfig,
    })
}

fn default_lxmd_config_dir() -> Result<PathBuf, String> {
    let system = PathBuf::from("/etc/lxmd");
    if system.join("config").exists() {
        return Ok(system);
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set; specify --config".to_string())?;
    let xdg = home.join(".config").join("lxmd");
    if xdg.join("config").exists() {
        return Ok(xdg);
    }

    Ok(home.join(".lxmd"))
}

fn apply_python_config_file(
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
        write_generated_reticulumd_config(paths.generated_rnsconfig.as_path(), &interfaces)?;
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
            .and_then(|value| parse_python_bool(value))
            .unwrap_or(false);
    }

    if let Some(propagation) = sections.get("propagation") {
        if let Some(enabled) =
            propagation.get("enable_node").and_then(|value| parse_python_bool(value))
        {
            effective.propagation_node = enabled;
        }
        effective.python_compat.auth_required = propagation
            .get("auth_required")
            .and_then(|value| parse_python_bool(value))
            .unwrap_or(false);
        effective.python_compat.autopeer =
            propagation.get("autopeer").and_then(|value| parse_python_bool(value)).unwrap_or(true);
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
            .and_then(|value| parse_python_bool(value))
            .unwrap_or(false);
        effective.python_compat.node_announce_at_start = propagation
            .get("announce_at_start")
            .and_then(|value| parse_python_bool(value))
            .unwrap_or(false);
        effective.python_compat.node_announce_interval_min =
            propagation.get("announce_interval").and_then(|value| value.parse::<u64>().ok());
        effective.python_compat.peer_announce_interval_min =
            propagation.get("peer_announce_interval").and_then(|value| value.parse::<u64>().ok());
    }

    effective.python_compat.allowed_identities =
        read_hash_list(paths.config_dir.join("allowed").as_path())?;
    effective.python_compat.ignored_destinations =
        read_hash_list(paths.config_dir.join("ignored").as_path())?;

    Ok(())
}

pub(crate) fn is_single_toml_config(path: &Path) -> Result<bool, String> {
    if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
        return Ok(false);
    }
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let value: toml::Value = toml::from_str(&contents)
        .map_err(|err| format!("invalid TOML in {}: {err}", path.display()))?;
    let Some(table) = value.as_table() else {
        return Ok(false);
    };
    if table.contains_key("lxmd") && table.len() == 1 {
        return Ok(false);
    }
    Ok(table.contains_key("node")
        || table.contains_key("rpc")
        || table.contains_key("transport")
        || table.contains_key("storage")
        || table.contains_key("propagation")
        || table.contains_key("lxmf")
        || table.contains_key("interfaces"))
}

fn apply_single_toml_config(
    effective: &mut crate::EffectiveArgs,
    config_path: &Path,
    paths: &crate::LxmdPaths,
) -> Result<(), String> {
    let contents = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read {}: {err}", config_path.display()))?;
    let config: crate::SingleTomlConfigFile = toml::from_str(&contents)
        .map_err(|err| format!("invalid TOML in {}: {err}", config_path.display()))?;

    effective.config_dir = Some(paths.config_dir.clone());
    effective.messages_dir = Some(paths.messages_dir.clone());
    effective.db = Some(resolve_config_path(
        config.storage.db.as_deref(),
        config_path,
        &paths.storage_dir.join("reticulum.db"),
    ));
    effective.identity = Some(resolve_config_path(
        config.storage.identity.as_deref(),
        config_path,
        &paths.identity_file,
    ));
    effective.rnsconfig = Some(paths.generated_rnsconfig.clone());

    if let Some(rpc) = config.rpc.listen {
        effective.rpc = rpc;
    }
    if let Some(transport) = config.transport.listen {
        effective.transport = Some(transport);
    }
    if let Some(display_name) = config
        .lxmf
        .display_name
        .clone()
        .or(config.node.display_name.clone())
        .filter(|value| !value.trim().is_empty())
    {
        effective.display_name = Some(display_name);
    }
    if let Some(enable) = config.propagation.enable {
        effective.propagation_node = enable;
    }
    if let Some(on_inbound) = config.lxmf.on_inbound.filter(|value| !value.trim().is_empty()) {
        effective.on_inbound = Some(on_inbound);
    }

    effective.python_compat.auth_required = config.propagation.auth_required.unwrap_or(false);
    effective.python_compat.autopeer = config.propagation.autopeer.unwrap_or(true);
    effective.python_compat.autopeer_maxdepth = config.propagation.autopeer_maxdepth.or(Some(6));
    effective.python_compat.max_peers = config.propagation.max_peers;
    effective.python_compat.from_static_only = config.propagation.from_static_only.unwrap_or(false);
    effective.python_compat.message_storage_limit_mb = config.propagation.message_storage_limit_mb;
    effective.python_compat.peering_cost = config.propagation.peering_cost;
    effective.python_compat.remote_peering_cost_max = config.propagation.remote_peering_cost_max;
    effective.python_compat.static_peers = config.propagation.static_peers.unwrap_or_default();
    effective.python_compat.control_allowed =
        config.propagation.control_allowed.unwrap_or_default();
    effective.python_compat.prioritised_destinations =
        config.propagation.prioritised_destinations.unwrap_or_default();
    effective.python_compat.node_announce_at_start =
        config.propagation.announce_at_start.unwrap_or(false);
    effective.python_compat.node_announce_interval_min = config.propagation.announce_interval;
    effective.python_compat.peer_announce_at_start = config.lxmf.announce_at_start.unwrap_or(false);
    effective.python_compat.delivery_transfer_max_kb =
        config.lxmf.delivery_transfer_max_accepted_size;

    write_generated_reticulumd_config(paths.generated_rnsconfig.as_path(), &config.interfaces)?;
    Ok(())
}

fn resolve_config_path(value: Option<&Path>, config_path: &Path, default: &Path) -> PathBuf {
    match value {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => config_path.parent().unwrap_or_else(|| Path::new(".")).join(path),
        None => default.to_path_buf(),
    }
}

fn write_generated_reticulumd_config(
    output_path: &Path,
    interfaces: &[crate::SingleTomlInterface],
) -> Result<(), String> {
    let mut output = String::new();
    for interface in interfaces {
        if !interface.enabled {
            continue;
        }
        output.push_str("[[interfaces]]\n");
        output.push_str(&format!("type = {:?}\n", interface.interface_type));
        output.push_str("enabled = true\n");
        if let Some(name) = interface.name.as_ref() {
            output.push_str(&format!("name = {:?}\n", name));
        }
        if let Some(host) = interface.host.as_ref() {
            output.push_str(&format!("host = {:?}\n", host));
        }
        if let Some(port) = interface.port {
            output.push_str(&format!("port = {port}\n"));
        }
        output.push('\n');
    }
    fs::write(output_path, output)
        .map_err(|err| format!("failed to write {}: {err}", output_path.display()))
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
            "enabled" => current.enabled = parse_python_bool(value),
            "target_host" | "host" => current.host = Some(value.to_string()),
            "target_port" | "listen_port" | "port" => {
                current.port = value.parse::<u16>().ok();
            }
            "listen_ip" => {
                if !value.is_empty() {
                    current.host = Some(value.to_string());
                }
            }
            _ => {}
        }
    }

    push_current(&mut parsed, current.take());
    for iface in &mut parsed {
        if iface.interface_type == "tcp_server"
            && iface.host.as_deref().map(str::trim).map_or(true, |value| value.is_empty())
        {
            iface.host = Some("0.0.0.0".to_string());
        }
    }
    parsed
}

fn strip_inline_comment(value: &str) -> &str {
    value.split(" #").next().unwrap_or(value)
}

fn parse_python_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
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

pub(crate) fn is_legacy_launcher_toml(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("toml")
}

pub(crate) fn apply_config_file(
    effective: &mut crate::EffectiveArgs,
    config_path: &Path,
) -> Result<(), String> {
    let contents = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read {}: {err}", config_path.display()))?;
    let parsed: crate::LxmdConfigFile = toml::from_str(&contents)
        .map_err(|err| format!("invalid TOML in {}: {err}", config_path.display()))?;
    let config = parsed.lxmd;
    if let Some(profile) = config.profile {
        effective.profile = profile;
    }
    if let Some(rpc) = config.rpc {
        effective.rpc = rpc;
    }
    if let Some(rnsconfig) = config.rnsconfig {
        effective.rnsconfig = Some(rnsconfig);
    }
    if let Some(propagation_node) = config.propagation_node {
        effective.propagation_node = propagation_node;
    }
    if let Some(on_inbound) = config.on_inbound {
        effective.on_inbound = Some(on_inbound);
    }
    if let Some(quiet) = config.quiet {
        effective.quiet = quiet;
    }
    if let Some(service) = config.service {
        effective.service = service;
    }
    if let Some(display_name) = config.display_name {
        effective.display_name = Some(display_name);
    }
    if let Some(db) = config.db {
        effective.db = Some(db);
    }
    if let Some(identity) = config.identity {
        effective.identity = Some(identity);
    }
    if let Some(transport) = config.transport {
        effective.transport = Some(transport);
    }
    if let Some(reticulumd) = config.reticulumd {
        effective.reticulumd = Some(reticulumd);
    }
    Ok(())
}
