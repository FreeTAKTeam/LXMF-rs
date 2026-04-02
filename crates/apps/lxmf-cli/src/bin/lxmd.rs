use clap::Parser;
use config::{is_legacy_launcher_toml, is_single_toml_config, load_effective_args};
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant};

#[path = "lxmd/config.rs"]
mod config;
#[path = "lxmd/inbound.rs"]
mod inbound;
#[path = "lxmd/query.rs"]
mod query;
#[path = "lxmd/rpc_client.rs"]
mod rpc_client;

const DEFAULT_RPC_ADDR: &str = "127.0.0.1:4243";
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const GENERATED_RETICULUMD_CONFIG: &str = "reticulumd.generated.toml";
const PYTHON_DEFAULT_LXMD_CONFIG: &str = r#"# This is an example LXM Daemon config file.

[propagation]
enable_node = no
announce_interval = 360
announce_at_start = yes
autopeer = yes
autopeer_maxdepth = 6
auth_required = no

[lxmf]
display_name = Anonymous Peer
announce_at_start = no
delivery_transfer_max_accepted_size = 1000
# on_inbound = rm

[logging]
loglevel = 4
"#;
const SINGLE_TOML_DEFAULT_CONFIG: &str = r#"[node]
display_name = "Rust LXMF Node"

[rpc]
listen = "127.0.0.1:4243"

[transport]
listen = "0.0.0.0:37428"

[storage]
db = "./storage/reticulum.db"
identity = "./identity"

[propagation]
enable = true
announce_at_start = true
announce_interval = 60
autopeer = true
autopeer_maxdepth = 6

[lxmf]
announce_at_start = true

[[interfaces]]
type = "tcp_client"
enabled = true
name = "rmap.world"
host = "rmap.world"
port = 4242
"#;

#[derive(Parser, Debug)]
#[command(name = "lxmd", about = "LXMF daemon compatibility entrypoint", version)]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,

    #[arg(long)]
    rnsconfig: Option<PathBuf>,

    #[arg(short = 'p', long = "propagation-node", default_value_t = false)]
    propagation_node: bool,

    #[arg(short = 'i', long = "on-inbound")]
    on_inbound: Option<String>,

    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,

    #[arg(short = 's', long, default_value_t = false)]
    service: bool,

    #[arg(long, default_value_t = false)]
    status: bool,

    #[arg(long, default_value_t = false)]
    peers: bool,

    #[arg(long)]
    sync: Option<String>,

    #[arg(short = 'b', long = "break")]
    unpeer: Option<String>,

    #[arg(long)]
    timeout: Option<f64>,

    #[arg(short = 'r', long)]
    remote: Option<String>,

    #[arg(long)]
    identity: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    exampleconfig: bool,

    #[arg(long, default_value = "default")]
    profile: String,

    #[arg(long)]
    rpc: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LxmdConfigFile {
    #[serde(default)]
    lxmd: LxmdConfigSection,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LxmdConfigSection {
    profile: Option<String>,
    rpc: Option<String>,
    rnsconfig: Option<PathBuf>,
    propagation_node: Option<bool>,
    on_inbound: Option<String>,
    quiet: Option<bool>,
    service: Option<bool>,
    display_name: Option<String>,
    db: Option<PathBuf>,
    identity: Option<PathBuf>,
    transport: Option<String>,
    reticulumd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct EffectiveArgs {
    profile: String,
    rpc: String,
    rnsconfig: Option<PathBuf>,
    propagation_node: bool,
    on_inbound: Option<String>,
    quiet: bool,
    service: bool,
    display_name: Option<String>,
    db: Option<PathBuf>,
    identity: Option<PathBuf>,
    transport: Option<String>,
    reticulumd: Option<PathBuf>,
    messages_dir: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    timeout_secs: f64,
    status: bool,
    peers: bool,
    sync: Option<String>,
    unpeer: Option<String>,
    remote: Option<String>,
    query_identity: Option<PathBuf>,
    python_compat: PythonCompatConfig,
}

#[derive(Debug, Clone)]
struct LxmdPaths {
    config_dir: PathBuf,
    config_file: PathBuf,
    identity_file: PathBuf,
    storage_dir: PathBuf,
    messages_dir: PathBuf,
    generated_rnsconfig: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlConfigFile {
    #[serde(default)]
    node: SingleTomlNode,
    #[serde(default)]
    rpc: SingleTomlRpc,
    #[serde(default)]
    transport: SingleTomlTransport,
    #[serde(default)]
    storage: SingleTomlStorage,
    #[serde(default)]
    propagation: SingleTomlPropagation,
    #[serde(default)]
    lxmf: SingleTomlLxmf,
    #[serde(default)]
    interfaces: Vec<SingleTomlInterface>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlNode {
    display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlRpc {
    listen: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlTransport {
    listen: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlStorage {
    db: Option<PathBuf>,
    identity: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlPropagation {
    enable: Option<bool>,
    announce_at_start: Option<bool>,
    announce_interval: Option<u64>,
    autopeer: Option<bool>,
    autopeer_maxdepth: Option<u32>,
    auth_required: Option<bool>,
    max_peers: Option<u32>,
    from_static_only: Option<bool>,
    message_storage_limit_mb: Option<u64>,
    peering_cost: Option<u32>,
    remote_peering_cost_max: Option<u32>,
    static_peers: Option<Vec<String>>,
    control_allowed: Option<Vec<String>>,
    prioritised_destinations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlLxmf {
    announce_at_start: Option<bool>,
    on_inbound: Option<String>,
    display_name: Option<String>,
    delivery_transfer_max_accepted_size: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SingleTomlInterface {
    #[serde(rename = "type")]
    interface_type: String,
    #[serde(default = "default_true_bool")]
    enabled: bool,
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
}

fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Clone)]
struct PythonCompatConfig {
    auth_required: bool,
    autopeer: bool,
    autopeer_maxdepth: Option<u32>,
    allowed_identities: Vec<String>,
    ignored_destinations: Vec<String>,
    prioritised_destinations: Vec<String>,
    control_allowed: Vec<String>,
    static_peers: Vec<String>,
    node_name: Option<String>,
    message_storage_limit_mb: Option<u64>,
    propagation_message_max_kb: Option<f64>,
    propagation_sync_max_kb: Option<f64>,
    propagation_stamp_cost_target: Option<u32>,
    propagation_stamp_cost_flexibility: Option<u32>,
    peering_cost: Option<u32>,
    remote_peering_cost_max: Option<u32>,
    max_peers: Option<u32>,
    from_static_only: bool,
    peer_announce_at_start: bool,
    node_announce_at_start: bool,
    peer_announce_interval_min: Option<u64>,
    node_announce_interval_min: Option<u64>,
    delivery_transfer_max_kb: Option<f64>,
}

impl Default for PythonCompatConfig {
    fn default() -> Self {
        Self {
            auth_required: false,
            autopeer: true,
            autopeer_maxdepth: Some(6),
            allowed_identities: Vec::new(),
            ignored_destinations: Vec::new(),
            prioritised_destinations: Vec::new(),
            control_allowed: Vec::new(),
            static_peers: Vec::new(),
            node_name: None,
            message_storage_limit_mb: None,
            propagation_message_max_kb: None,
            propagation_sync_max_kb: None,
            propagation_stamp_cost_target: None,
            propagation_stamp_cost_flexibility: None,
            peering_cost: None,
            remote_peering_cost_max: None,
            max_peers: None,
            from_static_only: false,
            peer_announce_at_start: false,
            node_announce_at_start: false,
            peer_announce_interval_min: None,
            node_announce_interval_min: None,
            delivery_transfer_max_kb: None,
        }
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    if args.exampleconfig {
        print!("{}", example_config());
        return ExitCode::SUCCESS;
    }

    let effective = match load_effective_args(&args) {
        Ok(effective) => effective,
        Err(err) => {
            eprintln!("lxmd: failed to load config: {err}");
            return ExitCode::from(1);
        }
    };

    emit_compatibility_notes(&args, &effective);
    if let Some(exit_code) = maybe_handle_query_mode(&effective) {
        return exit_code;
    }
    let reticulumd = resolve_reticulumd_binary(effective.reticulumd.as_deref());
    let mut cmd = Command::new(&reticulumd);

    cmd.arg("--rpc").arg(&effective.rpc);
    if let Some(rnsconfig) = effective.rnsconfig.as_ref() {
        cmd.arg("--config").arg(rnsconfig);
    }
    if let Some(db) = effective.db.as_ref() {
        cmd.arg("--db").arg(db);
    }
    if let Some(identity) = effective.identity.as_ref() {
        cmd.arg("--identity").arg(identity);
    }
    if let Some(transport) = effective.transport.as_ref() {
        cmd.arg("--transport").arg(transport);
    }
    if let Some(display_name) = effective.display_name.as_ref() {
        cmd.env("LXMF_DISPLAY_NAME", display_name);
    }
    if effective.propagation_node {
        cmd.env("LXMD_PROPAGATION_NODE", "1");
    }
    cmd.env(
        "LXMD_PEER_ANNOUNCE_AT_START",
        if effective.python_compat.peer_announce_at_start { "1" } else { "0" },
    );
    cmd.env(
        "LXMD_NODE_ANNOUNCE_AT_START",
        if effective.python_compat.node_announce_at_start { "1" } else { "0" },
    );
    if let Some(interval_min) = effective.python_compat.peer_announce_interval_min {
        cmd.env("LXMD_PEER_ANNOUNCE_INTERVAL_SECS", interval_min.saturating_mul(60).to_string());
    }
    if let Some(interval_min) = effective.python_compat.node_announce_interval_min {
        cmd.env("LXMD_NODE_ANNOUNCE_INTERVAL_SECS", interval_min.saturating_mul(60).to_string());
    }
    if !effective.python_compat.control_allowed.is_empty() {
        cmd.env("LXMD_CONTROL_ALLOWED", effective.python_compat.control_allowed.join(","));
    }

    let rpc_addr = effective.rpc.as_str();
    if requires_supervised_launch(&effective) {
        return launch_supervised(cmd, reticulumd, rpc_addr, &effective);
    }

    #[cfg(unix)]
    {
        let err = cmd.exec();
        eprintln!("lxmd: failed to exec {}: {}", reticulumd.display(), err);
        ExitCode::from(1)
    }

    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
            Err(err) => {
                eprintln!("lxmd: failed to launch {}: {}", reticulumd.display(), err);
                ExitCode::from(1)
            }
        }
    }
}

fn requires_supervised_launch(args: &EffectiveArgs) -> bool {
    args.propagation_node || args.on_inbound.is_some()
}

fn maybe_handle_query_mode(args: &EffectiveArgs) -> Option<ExitCode> {
    if !(args.status || args.peers || args.sync.is_some() || args.unpeer.is_some()) {
        return None;
    }

    let rpc_addr = match query::resolve_query_rpc_addr(args.remote.as_deref(), &args.rpc) {
        Ok(rpc_addr) => rpc_addr,
        Err(err) => {
            eprintln!("lxmd: {err}");
            return Some(ExitCode::from(2));
        }
    };

    let result = if let Some(remote) =
        args.remote.as_deref().filter(|remote| !query::looks_like_rpc_addr(remote))
    {
        let identity_private_key_hex = match read_identity_private_key_hex(
            args.query_identity.as_deref().or(args.identity.as_deref()),
        ) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("lxmd: {err}");
                return Some(ExitCode::from(2));
            }
        };
        if let Some(peer) = args.sync.as_deref() {
            rpc_client::rpc_call(
                &rpc_addr,
                "propagation_remote_sync",
                Some(json!({
                    "remote": remote,
                    "peer": peer,
                    "identity_private_key_hex": identity_private_key_hex,
                    "timeout_secs": args.timeout_secs,
                })),
            )
            .map(|value| {
                println!("Sync requested for peer {peer} on remote node {remote}");
                value
            })
        } else if let Some(peer) = args.unpeer.as_deref() {
            rpc_client::rpc_call(
                &rpc_addr,
                "propagation_remote_unpeer",
                Some(json!({
                    "remote": remote,
                    "peer": peer,
                    "identity_private_key_hex": identity_private_key_hex,
                    "timeout_secs": args.timeout_secs,
                })),
            )
            .map(|value| {
                println!("Broke peering with {peer} on remote node {remote}");
                value
            })
        } else {
            query::show_remote_status_and_peers(
                &rpc_addr,
                remote,
                identity_private_key_hex.as_deref(),
                args.timeout_secs,
                args.status,
                args.peers,
            )
        }
    } else if let Some(peer) = args.sync.as_deref() {
        rpc_client::rpc_call(&rpc_addr, "peer_sync", Some(json!({ "peer": peer }))).map(|value| {
            println!("Sync requested for peer {peer}");
            value
        })
    } else if let Some(peer) = args.unpeer.as_deref() {
        rpc_client::rpc_call(&rpc_addr, "peer_unpeer", Some(json!({ "peer": peer }))).map(|value| {
            println!("Broke peering with {peer}");
            value
        })
    } else {
        query::show_status_and_peers(&rpc_addr, args.status, args.peers)
    };

    match result {
        Ok(_) => Some(ExitCode::SUCCESS),
        Err(err) => {
            eprintln!("lxmd: {err}");
            Some(ExitCode::from(1))
        }
    }
}

fn launch_supervised(
    mut cmd: Command,
    reticulumd: PathBuf,
    rpc_addr: &str,
    args: &EffectiveArgs,
) -> ExitCode {
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("lxmd: failed to launch {}: {}", reticulumd.display(), err);
            return ExitCode::from(1);
        }
    };

    if let Err(err) = wait_until_ready(&mut child, rpc_addr, READY_TIMEOUT) {
        eprintln!("lxmd: {err}");
        let _ = child.kill();
        let _ = child.wait();
        return ExitCode::from(1);
    }

    if args.propagation_node {
        if let Err(err) = enable_propagation_mode(rpc_addr) {
            eprintln!("lxmd: failed to enable propagation mode: {err}");
            let _ = child.kill();
            let _ = child.wait();
            return ExitCode::from(1);
        }
    }

    if let Err(err) = apply_python_compat_config(rpc_addr, args) {
        eprintln!("lxmd: failed to apply python-style daemon settings: {err}");
        let _ = child.kill();
        let _ = child.wait();
        return ExitCode::from(1);
    }

    if args.propagation_node {
        if let Err(err) = rpc_client::rpc_call(rpc_addr, "announce_now", None) {
            eprintln!("lxmd: failed to announce propagation state: {err}");
        }
    }

    if let Some(command) = args.on_inbound.clone() {
        rpc_client::spawn_on_inbound_watcher(
            rpc_addr.to_string(),
            command,
            args.messages_dir.clone(),
        );
    }

    match child.wait() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(err) => {
            eprintln!("lxmd: failed waiting for reticulumd: {}", err);
            ExitCode::from(1)
        }
    }
}

fn emit_compatibility_notes(args: &Args, effective: &EffectiveArgs) {
    for message in compatibility_notes(args, effective) {
        eprintln!("lxmd: {message}");
    }
}

fn compatibility_notes(args: &Args, effective: &EffectiveArgs) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(config) = args.config.as_ref() {
        let message = if is_single_toml_config(config).unwrap_or(false) {
            format!(
                "--config loaded single-file TOML settings for profile '{}' and rpc '{}'",
                effective.profile, effective.rpc
            )
        } else if is_legacy_launcher_toml(config) {
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
        notes.push("--service is accepted for compatibility and currently behaves the same as foreground mode".to_string());
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

fn example_config() -> &'static str {
    SINGLE_TOML_DEFAULT_CONFIG
}

fn resolve_reticulumd_binary(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }
    if let Some(path) = env::var_os("RETICULUMD_BIN").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    let current_exe = env::current_exe().ok();
    let mut candidates = Vec::new();
    if let Some(exe) = current_exe.as_ref() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(reticulumd_binary_name()));
            if let Some(parent) = dir.parent() {
                candidates.push(parent.join(reticulumd_binary_name()));
            }
        }
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from(reticulumd_binary_name()))
}

fn reticulumd_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "reticulumd.exe"
    }

    #[cfg(not(windows))]
    {
        "reticulumd"
    }
}

fn wait_until_ready(child: &mut Child, rpc_addr: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!("reticulumd exited before becoming ready: {}", status));
            }
            Ok(None) => {}
            Err(err) => return Err(format!("failed to check reticulumd status: {err}")),
        }

        match http_get_ready(rpc_addr) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => {}
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for reticulumd readiness at http://{rpc_addr}/readyz"
            ));
        }
        thread::sleep(READY_POLL_INTERVAL);
    }
}

fn enable_propagation_mode(rpc_addr: &str) -> Result<(), String> {
    let response = rpc_client::rpc_call(
        rpc_addr,
        "propagation_enable",
        Some(json!({
            "enabled": true,
        })),
    )?;
    if let Some(error) = response.get("error").and_then(|value| value.as_object()) {
        let message =
            error.get("message").and_then(|value| value.as_str()).unwrap_or("unknown rpc error");
        return Err(message.to_string());
    }
    Ok(())
}

fn apply_python_compat_config(rpc_addr: &str, args: &EffectiveArgs) -> Result<(), String> {
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

fn http_get_ready(rpc_addr: &str) -> Result<bool, String> {
    let response = rpc_client::http_request_bytes(
        rpc_addr,
        format!("GET /readyz HTTP/1.1\r\nHost: {rpc_addr}\r\nConnection: close\r\n\r\n").as_bytes(),
    )?;
    Ok(response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200"))
}

fn read_identity_private_key_hex(path: Option<&Path>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read identity {}: {err}", path.display()))?;
    Ok(Some(hex::encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::config::{
        apply_config_file, parse_python_lxmd_config, parse_python_reticulum_interfaces,
        prepare_lxmd_paths,
    };
    use super::{
        compatibility_notes, is_single_toml_config, load_effective_args,
        requires_supervised_launch, EffectiveArgs,
    };
    use clap::Parser;
    use serde_json::json;
    use std::fs;

    #[test]
    fn compatibility_notes_only_emitted_for_used_flags() {
        let args = super::Args::parse_from(["lxmd", "--propagation-node", "--service"]);
        let effective = load_effective_args(&args).expect("effective args");
        let notes = compatibility_notes(&args, &effective);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn propagation_node_uses_supervised_launch() {
        let args = super::Args::parse_from(["lxmd", "--propagation-node"]);
        let effective = load_effective_args(&args).expect("effective args");
        assert!(requires_supervised_launch(&effective));
    }

    #[test]
    fn rpc_frame_roundtrips() {
        let value = json!({
            "id": 1,
            "result": {
                "propagation": {
                    "enabled": true
                }
            }
        });
        let encoded = super::rpc_client::encode_rpc_frame(value.clone()).expect("encode");
        let decoded = super::rpc_client::decode_rpc_frame(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn config_file_applies_lxmd_launcher_settings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("lxmd.toml");
        fs::write(
            &path,
            r#"[lxmd]
profile = "alpha"
rpc = "127.0.0.1:5555"
rnsconfig = "/tmp/reticulum.toml"
propagation_node = true
on_inbound = "echo hi"
display_name = "node-a"
db = "/tmp/reticulum.db"
identity = "/tmp/identity"
transport = "127.0.0.1:4242"
reticulumd = "/tmp/reticulumd"
"#,
        )
        .expect("write config");

        let mut effective = EffectiveArgs {
            profile: "default".into(),
            rpc: "127.0.0.1:4243".into(),
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
            python_compat: super::PythonCompatConfig::default(),
        };
        apply_config_file(&mut effective, &path).expect("apply config");
        assert_eq!(effective.profile, "alpha");
        assert_eq!(effective.rpc, "127.0.0.1:5555");
        assert!(effective.propagation_node);
        assert_eq!(effective.on_inbound.as_deref(), Some("echo hi"));
        assert_eq!(effective.display_name.as_deref(), Some("node-a"));
        assert_eq!(effective.transport.as_deref(), Some("127.0.0.1:4242"));
    }

    #[test]
    fn single_toml_config_is_detected_and_loaded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("lxmd.toml");
        fs::write(
            &path,
            r#"[node]
display_name = "Node A"

[rpc]
listen = "127.0.0.1:5555"

[transport]
listen = "0.0.0.0:37777"

[storage]
db = "./state/reticulum.db"
identity = "./state/identity"

[propagation]
enable = true
announce_at_start = true
announce_interval = 90
autopeer = true
autopeer_maxdepth = 7

[lxmf]
on_inbound = "echo hi"

[[interfaces]]
type = "tcp_client"
enabled = true
name = "rmap.world"
host = "rmap.world"
port = 4242
"#,
        )
        .expect("write config");

        assert!(is_single_toml_config(&path).expect("detect single toml"));
        let args = super::Args::parse_from(["lxmd", "--config", path.to_str().expect("utf8 path")]);
        let effective = load_effective_args(&args).expect("effective args");
        assert_eq!(effective.rpc, "127.0.0.1:5555");
        assert_eq!(effective.transport.as_deref(), Some("0.0.0.0:37777"));
        assert_eq!(effective.display_name.as_deref(), Some("Node A"));
        assert!(effective.propagation_node);
        assert_eq!(effective.on_inbound.as_deref(), Some("echo hi"));
        assert_eq!(effective.python_compat.autopeer_maxdepth, Some(7));
        assert_eq!(effective.db.as_deref(), Some(temp.path().join("state/reticulum.db").as_path()));
        assert_eq!(
            effective.identity.as_deref(),
            Some(temp.path().join("state/identity").as_path())
        );
        let generated = temp.path().join(super::GENERATED_RETICULUMD_CONFIG);
        let generated_contents =
            fs::read_to_string(&generated).expect("generated reticulum config");
        assert!(generated_contents.contains("host = \"rmap.world\""));
        assert!(generated_contents.contains("port = 4242"));
    }

    #[test]
    fn python_reticulum_interfaces_parse_tcp_server_and_client() {
        let interfaces = parse_python_reticulum_interfaces(
            r#"
[interfaces]
  [[Server]]
    type = TCPServerInterface
    enabled = yes
    listen_ip = 0.0.0.0
    listen_port = 4242

  [[Client]]
    type = TCPClientInterface
    enabled = yes
    target_host = rmap.world
    target_port = 4243
"#,
        );
        assert_eq!(interfaces.len(), 2);
        assert_eq!(interfaces[0].interface_type, "tcp_server");
        assert_eq!(interfaces[0].host.as_deref(), Some("0.0.0.0"));
        assert_eq!(interfaces[0].port, Some(4242));
        assert_eq!(interfaces[1].interface_type, "tcp_client");
        assert_eq!(interfaces[1].host.as_deref(), Some("rmap.world"));
        assert_eq!(interfaces[1].port, Some(4243));
    }

    #[test]
    fn python_config_generates_reticulumd_interfaces_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("lxmd");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        let config_path = config_dir.join("config");
        fs::write(
            &config_path,
            r#"
[propagation]
enable_node = yes

[interfaces]
  [[Server]]
    type = TCPServerInterface
    enabled = yes
    listen_port = 4242
"#,
        )
        .expect("write config");

        let args =
            super::Args::parse_from(["lxmd", "--config", config_path.to_str().expect("utf8 path")]);
        let effective = load_effective_args(&args).expect("effective args");
        let generated = config_dir.join(super::GENERATED_RETICULUMD_CONFIG);
        let generated_contents = fs::read_to_string(&generated).expect("generated config");

        assert_eq!(effective.rnsconfig.as_deref(), Some(generated.as_path()));
        assert!(generated_contents.contains("type = \"tcp_server\""));
        assert!(generated_contents.contains("host = \"0.0.0.0\""));
        assert!(generated_contents.contains("port = 4242"));
    }

    #[test]
    fn python_config_sections_are_parsed() {
        let sections = parse_python_lxmd_config(
            r#"
[propagation]
enable_node = yes

[lxmf]
display_name = Anonymous Peer
on_inbound = rm
"#,
        );
        assert_eq!(sections["propagation"]["enable_node"], "yes");
        assert_eq!(sections["lxmf"]["display_name"], "Anonymous Peer");
        assert_eq!(sections["lxmf"]["on_inbound"], "rm");
    }

    #[test]
    fn prepare_lxmd_paths_creates_python_style_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("lxmd");
        let paths = prepare_lxmd_paths(Some(&config_dir)).expect("paths");
        assert!(paths.config_file.exists());
        assert!(paths.messages_dir.exists());
        assert_eq!(paths.identity_file, config_dir.join("identity"));
    }
}
