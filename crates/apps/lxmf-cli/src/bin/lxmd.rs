use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

#[derive(Debug, Serialize)]
struct SavedMessageContainer<'a> {
    state: u8,
    #[serde(with = "serde_bytes")]
    lxmf_bytes: &'a [u8],
    transport_encrypted: bool,
    transport_encryption: Option<&'a str>,
    method: u8,
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

    let rpc_addr = match resolve_query_rpc_addr(args) {
        Ok(rpc_addr) => rpc_addr,
        Err(err) => {
            eprintln!("lxmd: {err}");
            return Some(ExitCode::from(2));
        }
    };

    let result = if let Some(remote) =
        args.remote.as_deref().filter(|remote| !looks_like_rpc_addr(remote))
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
            rpc_call(
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
            rpc_call(
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
            show_remote_status_and_peers(
                &rpc_addr,
                remote,
                identity_private_key_hex.as_deref(),
                args,
            )
        }
    } else if let Some(peer) = args.sync.as_deref() {
        rpc_call(&rpc_addr, "peer_sync", Some(json!({ "peer": peer }))).map(|value| {
            println!("Sync requested for peer {peer}");
            value
        })
    } else if let Some(peer) = args.unpeer.as_deref() {
        rpc_call(&rpc_addr, "peer_unpeer", Some(json!({ "peer": peer }))).map(|value| {
            println!("Broke peering with {peer}");
            value
        })
    } else {
        show_status_and_peers(&rpc_addr, args)
    };

    match result {
        Ok(_) => Some(ExitCode::SUCCESS),
        Err(err) => {
            eprintln!("lxmd: {err}");
            Some(ExitCode::from(1))
        }
    }
}

fn resolve_query_rpc_addr(args: &EffectiveArgs) -> Result<String, String> {
    match args.remote.as_deref() {
        Some(remote) if looks_like_rpc_addr(remote) => Ok(remote.to_string()),
        Some(_) => Ok(args.rpc.clone()),
        None => Ok(args.rpc.clone()),
    }
}

fn looks_like_rpc_addr(value: &str) -> bool {
    value.contains(':') || value.starts_with("http://") || value.starts_with("https://")
}

fn show_status_and_peers(
    rpc_addr: &str,
    args: &EffectiveArgs,
) -> Result<serde_json::Value, String> {
    let status = unwrap_rpc_result(
        rpc_call(rpc_addr, "daemon_status_ex", None)
            .or_else(|_| rpc_call(rpc_addr, "propagation_status", None))?,
    );
    let peers = unwrap_rpc_result(
        rpc_call(rpc_addr, "list_peers", None).unwrap_or_else(|_| json!({ "peers": [] })),
    );

    if args.status {
        print_status_summary(&status, &peers);
    }
    if args.peers {
        print_peer_summary(&peers);
    }
    if !args.status && !args.peers {
        print_status_summary(&status, &peers);
    }

    Ok(json!({
        "status": status,
        "peers": peers,
    }))
}

fn show_remote_status_and_peers(
    rpc_addr: &str,
    remote: &str,
    identity_private_key_hex: Option<&str>,
    args: &EffectiveArgs,
) -> Result<serde_json::Value, String> {
    let response = unwrap_rpc_result(rpc_call(
        rpc_addr,
        "propagation_remote_status",
        Some(json!({
            "remote": remote,
            "identity_private_key_hex": identity_private_key_hex,
            "timeout_secs": args.timeout_secs,
        })),
    )?);
    let status = response.get("status").cloned().unwrap_or(response.clone());

    if args.status || (!args.status && !args.peers) {
        print_remote_status_summary(&status);
    }
    if args.peers {
        print_remote_peer_summary(&status);
    }

    Ok(status)
}

fn unwrap_rpc_result(value: serde_json::Value) -> serde_json::Value {
    value.get("result").cloned().unwrap_or(value)
}

fn print_status_summary(status: &serde_json::Value, peers: &serde_json::Value) {
    let propagation = status.get("propagation").unwrap_or(status);
    let delivery_policy = status.get("delivery_policy");
    let stamp_policy = status.get("stamp_policy");
    let enabled = propagation.get("enabled").and_then(|value| value.as_bool()).unwrap_or(false);
    let target_cost = propagation.get("target_cost").and_then(|value| value.as_u64()).unwrap_or(0);
    let total_ingested =
        propagation.get("total_ingested").and_then(|value| value.as_u64()).unwrap_or(0);
    let selected_node =
        propagation.get("selected_node").and_then(|value| value.as_str()).unwrap_or("<none>");
    let peer_count =
        peers.get("peers").and_then(|value| value.as_array()).map(|items| items.len()).unwrap_or(0);
    let auth_required = delivery_policy
        .and_then(|policy| policy.get("auth_required"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let ignored_count = delivery_policy
        .and_then(|policy| policy.get("ignored_destinations"))
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let prioritised_count = delivery_policy
        .and_then(|policy| policy.get("prioritised_destinations"))
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let flexibility = stamp_policy
        .and_then(|policy| policy.get("flexibility"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    println!();
    println!("LXMF Propagation Node {}", if enabled { "enabled" } else { "disabled" });
    println!("Peers: {peer_count}");
    println!("Target stamp cost: {target_cost}");
    println!("Stamp cost flexibility: {flexibility}");
    println!("Authentication required: {}", if auth_required { "yes" } else { "no" });
    println!("Ignored destinations: {ignored_count}");
    println!("Prioritised destinations: {prioritised_count}");
    println!("Total ingested messages: {total_ingested}");
    println!("Selected outbound propagation node: {selected_node}");
}

fn print_peer_summary(peers: &serde_json::Value) {
    println!();
    let items = peers.get("peers").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    if items.is_empty() {
        println!("No peers discovered");
        return;
    }

    for peer in items {
        let id = peer.get("peer").and_then(|value| value.as_str()).unwrap_or("<unknown>");
        let name = peer.get("name").and_then(|value| value.as_str()).unwrap_or("");
        let last_seen = peer.get("last_seen").and_then(|value| value.as_i64()).unwrap_or(0);
        if name.is_empty() {
            println!("{id} last_seen={last_seen}");
        } else {
            println!("{id} name=\"{name}\" last_seen={last_seen}");
        }
    }
}

fn print_remote_status_summary(status: &serde_json::Value) {
    let total_peers = status.get("total_peers").and_then(|value| value.as_u64()).unwrap_or(0);
    let discovered_peers =
        status.get("discovered_peers").and_then(|value| value.as_u64()).unwrap_or(0);
    let target_cost = status.get("target_stamp_cost").and_then(|value| value.as_u64()).unwrap_or(0);
    let flexibility =
        status.get("stamp_cost_flexibility").and_then(|value| value.as_u64()).unwrap_or(0);
    let max_peers = status.get("max_peers").and_then(|value| value.as_u64()).unwrap_or(0);
    let destination =
        status.get("destination_hash").and_then(|value| value.as_str()).unwrap_or("<unknown>");

    println!();
    println!("Remote LXMF Propagation Node status");
    println!("Destination hash: {destination}");
    println!("Peers: {total_peers}");
    println!("Discovered peers: {discovered_peers}");
    println!("Max peers: {max_peers}");
    println!("Target stamp cost: {target_cost}");
    println!("Stamp cost flexibility: {flexibility}");
}

fn print_remote_peer_summary(status: &serde_json::Value) {
    println!();
    let Some(peers) = status.get("peers").and_then(|value| value.as_object()) else {
        println!("No peers discovered");
        return;
    };
    if peers.is_empty() {
        println!("No peers discovered");
        return;
    }

    let mut rows: Vec<_> = peers.iter().collect();
    rows.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (peer, details) in rows {
        let name = details.get("name").and_then(|value| value.as_str()).unwrap_or("");
        let last_heard = details.get("last_heard").and_then(|value| value.as_i64()).unwrap_or(0);
        if name.is_empty() {
            println!("{peer} last_seen={last_heard}");
        } else {
            println!("{peer} name=\"{name}\" last_seen={last_heard}");
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
        if let Err(err) = rpc_call(rpc_addr, "announce_now", None) {
            eprintln!("lxmd: failed to announce propagation state: {err}");
        }
    }

    if let Some(command) = args.on_inbound.clone() {
        spawn_on_inbound_watcher(rpc_addr.to_string(), command, args.messages_dir.clone());
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
        if args.remote.as_ref().is_some_and(|remote| looks_like_rpc_addr(remote)) {
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
    let response = rpc_call(
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
        rpc_call(
            rpc_addr,
            "set_delivery_policy",
            Some(serde_json::Value::Object(delivery_params)),
        )?;
    }

    if compat.propagation_stamp_cost_target.is_some()
        || compat.propagation_stamp_cost_flexibility.is_some()
    {
        rpc_call(
            rpc_addr,
            "stamp_policy_set",
            Some(json!({
                "target_cost": compat.propagation_stamp_cost_target,
                "flexibility": compat.propagation_stamp_cost_flexibility,
            })),
        )?;
    }

    if args.propagation_node || compat.propagation_stamp_cost_target.is_some() {
        rpc_call(
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
            let _ = rpc_call(rpc_addr, "peer_sync", Some(json!({ "peer": peer })));
        }
    }

    Ok(())
}

fn http_get_ready(rpc_addr: &str) -> Result<bool, String> {
    let response = http_request_bytes(
        rpc_addr,
        format!("GET /readyz HTTP/1.1\r\nHost: {rpc_addr}\r\nConnection: close\r\n\r\n").as_bytes(),
    )?;
    Ok(response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200"))
}

fn spawn_on_inbound_watcher(rpc_addr: String, command: String, messages_dir: Option<PathBuf>) {
    thread::spawn(move || {
        let mut cursor: Option<String> = None;
        loop {
            match poll_event_batch(&rpc_addr, cursor.as_deref()) {
                Ok((events, next_cursor)) => {
                    cursor = next_cursor;
                    if events.is_empty() {
                        thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                    for event in events {
                        if let Err(err) =
                            run_on_inbound_command(&command, &event, messages_dir.as_deref())
                        {
                            eprintln!("lxmd: on-inbound hook failed: {err}");
                        }
                    }
                }
                Err(err)
                    if err.contains("SDK_RUNTIME_CURSOR_EXPIRED")
                        || err.contains("SDK_RUNTIME_STREAM_DEGRADED") =>
                {
                    cursor = None;
                    thread::sleep(Duration::from_millis(250));
                }
                Err(err) => {
                    eprintln!("lxmd: inbound event watcher stopped: {err}");
                    break;
                }
            }
        }
    });
}

fn poll_event_batch(
    rpc_addr: &str,
    cursor: Option<&str>,
) -> Result<(Vec<serde_json::Value>, Option<String>), String> {
    let response = rpc_call(
        rpc_addr,
        "sdk_poll_events_v2",
        Some(json!({
            "cursor": cursor,
            "max": 256,
        })),
    )?;
    let result = response.get("result").unwrap_or(&response);
    if let Some(error) = response.get("error").or_else(|| result.get("error")) {
        let code = error.get("code").and_then(|value| value.as_str()).unwrap_or("RPC_ERROR");
        let message =
            error.get("message").and_then(|value| value.as_str()).unwrap_or("unknown rpc error");
        return Err(format!("{code}: {message}"));
    }
    let events =
        result.get("events").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let next_cursor =
        result.get("next_cursor").and_then(|value| value.as_str()).map(ToOwned::to_owned);
    Ok((events, next_cursor))
}

fn rpc_call(
    rpc_addr: &str,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let payload = encode_rpc_frame(json!({
        "id": 1u64,
        "method": method,
        "params": params,
    }))?;
    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: {rpc_addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let mut request_bytes = request.into_bytes();
    request_bytes.extend_from_slice(&payload);
    let response = http_request_bytes(rpc_addr, &request_bytes)?;
    let body = http_body(&response).ok_or_else(|| "rpc response missing body".to_string())?;
    if let Some(status) = http_status_code(&response) {
        if status >= 400 && !looks_like_rpc_frame(body) {
            let message = String::from_utf8_lossy(body).trim().to_string();
            return Err(if message.is_empty() {
                format!("rpc http error {status}")
            } else {
                message
            });
        }
    }
    decode_rpc_frame(body)
}

fn http_request_bytes(rpc_addr: &str, request: &[u8]) -> Result<Vec<u8>, String> {
    let addr = resolve_socket_addr(rpc_addr)?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|err| err.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|err| err.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|err| err.to_string())?;
    stream.write_all(request).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    Ok(bytes)
}

fn resolve_socket_addr(rpc_addr: &str) -> Result<SocketAddr, String> {
    rpc_addr
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .next()
        .ok_or_else(|| format!("failed to resolve rpc address {rpc_addr}"))
}

fn http_body(response: &[u8]) -> Option<&[u8]> {
    response.windows(4).position(|window| window == b"\r\n\r\n").map(|index| &response[index + 4..])
}

fn http_status_code(response: &[u8]) -> Option<u16> {
    let header_end = response.windows(2).position(|window| window == b"\r\n")?;
    let status_line = std::str::from_utf8(&response[..header_end]).ok()?;
    let mut parts = status_line.split_whitespace();
    let _http = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

fn looks_like_rpc_frame(body: &[u8]) -> bool {
    if body.len() < 4 {
        return false;
    }
    let len = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    body.len() >= len + 4
}

fn encode_rpc_frame(value: serde_json::Value) -> Result<Vec<u8>, String> {
    let payload = rmp_serde::to_vec(&value).map_err(|err| err.to_string())?;
    let len = u32::try_from(payload.len()).map_err(|_| "rpc frame too large".to_string())?;
    let mut framed = Vec::with_capacity(payload.len() + 4);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

fn decode_rpc_frame(bytes: &[u8]) -> Result<serde_json::Value, String> {
    if bytes.len() < 4 {
        return Err("rpc response too short".to_string());
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < len + 4 {
        return Err("rpc response incomplete".to_string());
    }
    let value: serde_json::Value =
        rmp_serde::from_slice(&bytes[4..4 + len]).map_err(|err| err.to_string())?;
    Ok(normalize_rpc_response(value))
}

fn normalize_rpc_response(value: serde_json::Value) -> serde_json::Value {
    let Some(items) = value.as_array() else {
        return value;
    };
    if items.len() != 3 {
        return value;
    }

    let id = items.first().cloned().unwrap_or(serde_json::Value::Null);
    let result = items.get(1).cloned().unwrap_or(serde_json::Value::Null);
    let error = items.get(2).cloned().unwrap_or(serde_json::Value::Null);
    let mut map = serde_json::Map::new();
    map.insert("id".to_string(), id);
    if !result.is_null() {
        map.insert("result".to_string(), result);
    }
    let error = normalize_rpc_error(error);
    if !error.is_null() {
        map.insert("error".to_string(), error);
    }
    serde_json::Value::Object(map)
}

fn normalize_rpc_error(value: serde_json::Value) -> serde_json::Value {
    let Some(items) = value.as_array() else {
        return value;
    };
    if items.is_empty() {
        return serde_json::Value::Null;
    }

    json!({
        "code": items.first().and_then(|entry| entry.as_str()).unwrap_or_default(),
        "message": items.get(1).and_then(|entry| entry.as_str()).unwrap_or_default(),
        "machine_code": items.get(2).cloned().unwrap_or(serde_json::Value::Null),
        "category": items.get(3).cloned().unwrap_or(serde_json::Value::Null),
        "retryable": items.get(4).cloned().unwrap_or(serde_json::Value::Null),
        "is_user_actionable": items.get(5).cloned().unwrap_or(serde_json::Value::Null),
    })
}

fn run_on_inbound_command(
    command: &str,
    event: &serde_json::Value,
    messages_dir: Option<&Path>,
) -> Result<(), String> {
    let event_type =
        event.get("event_type").and_then(|value| value.as_str()).unwrap_or("<unknown>");
    if event_type != "inbound" {
        return Ok(());
    }

    let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
    let message = payload.get("message").cloned().unwrap_or_else(|| json!({}));
    let body = serde_json::to_vec(&payload).map_err(|err| err.to_string())?;
    let message_path = write_inbound_message_file(messages_dir, &payload, &message)?;

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_command = if let Some(path) = message_path.as_ref() {
        format!("{command} \"{}\"", shell_escape(path))
    } else {
        command.to_string()
    };
    let mut child = Command::new(shell)
        .arg("-c")
        .arg(shell_command)
        .env("LXMD_EVENT_TYPE", "inbound")
        .env("LXMD_EVENT_JSON", compact_json(&payload)?)
        .env("LXMD_MESSAGE_JSON", compact_json(&message)?)
        .env("LXMD_MESSAGE_ID", json_env(&message, "id"))
        .env("LXMD_MESSAGE_SOURCE", json_env(&message, "source"))
        .env("LXMD_MESSAGE_DESTINATION", json_env(&message, "destination"))
        .env("LXMD_MESSAGE_TITLE", json_env(&message, "title"))
        .env("LXMD_MESSAGE_CONTENT", json_env(&message, "content"))
        .env("LXMD_MESSAGE_TIMESTAMP", json_env(&message, "timestamp"))
        .env(
            "LXMD_MESSAGE_PATH",
            message_path.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| err.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&body).map_err(|err| err.to_string())?;
    }

    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command exited with status {status}"))
    }
}

fn json_env(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) if !other.is_null() => other.to_string(),
        _ => String::new(),
    }
}

fn compact_json(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|err| err.to_string())
}

fn read_identity_private_key_hex(path: Option<&Path>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read identity {}: {err}", path.display()))?;
    Ok(Some(hex::encode(bytes)))
}

fn write_inbound_message_file(
    messages_dir: Option<&Path>,
    payload: &serde_json::Value,
    message: &serde_json::Value,
) -> Result<Option<PathBuf>, String> {
    let Some(messages_dir) = messages_dir else {
        return Ok(None);
    };
    fs::create_dir_all(messages_dir).map_err(|err| err.to_string())?;
    let message_id = json_env(message, "id");
    let file_name = if message_id.is_empty() {
        format!("{}.json", now_epoch_millis())
    } else {
        sanitize_file_name(&message_id)
    };
    let path = messages_dir.join(file_name);
    let packed = pack_saved_inbound_message(payload, message)?;
    fs::write(&path, packed).map_err(|err| err.to_string())?;
    Ok(Some(path))
}

fn sanitize_file_name(input: &str) -> String {
    input
        .chars()
        .map(
            |ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                    ch
                } else {
                    '_'
                }
            },
        )
        .collect()
}

fn shell_escape(path: &Path) -> String {
    path.display().to_string().replace('"', "\\\"")
}

fn now_epoch_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

fn pack_saved_inbound_message(
    payload: &serde_json::Value,
    message: &serde_json::Value,
) -> Result<Vec<u8>, String> {
    let lxmf_bytes =
        if let Some(raw_hex) = payload.get("lxmf_bytes_hex").and_then(|value| value.as_str()) {
            hex::decode(raw_hex).map_err(|err| err.to_string())?
        } else {
            reconstruct_inbound_wire_bytes(message)?
        };
    let container = SavedMessageContainer {
        state: 0x00,
        lxmf_bytes: &lxmf_bytes,
        transport_encrypted: false,
        transport_encryption: None,
        method: 0x00,
    };
    let mut out = Vec::new();
    let mut serializer = rmp_serde::Serializer::new(&mut out).with_struct_map();
    container.serialize(&mut serializer).map_err(|err| err.to_string())?;
    Ok(out)
}

fn reconstruct_inbound_wire_bytes(message: &serde_json::Value) -> Result<Vec<u8>, String> {
    let destination = decode_hash_field(message, "destination")?;
    let source = decode_hash_field(message, "source")?;
    let timestamp = message.get("timestamp").and_then(|value| value.as_i64()).unwrap_or(0);
    let title = message.get("title").and_then(|value| value.as_str()).unwrap_or("");
    let content = message.get("content").and_then(|value| value.as_str()).unwrap_or("");
    let fields = message.get("fields").map(json_to_rmpv).transpose()?.unwrap_or(rmpv::Value::Nil);
    let payload = rmpv::Value::Array(vec![
        rmpv::Value::from(timestamp),
        rmpv::Value::from(title),
        rmpv::Value::from(content),
        fields,
    ]);
    let packed_payload = rmp_serde::to_vec(&payload).map_err(|err| err.to_string())?;
    let mut wire = Vec::with_capacity(16 + 16 + 64 + packed_payload.len());
    wire.extend_from_slice(&destination);
    wire.extend_from_slice(&source);
    wire.extend_from_slice(&[0u8; 64]);
    wire.extend_from_slice(&packed_payload);
    Ok(wire)
}

fn decode_hash_field(message: &serde_json::Value, key: &str) -> Result<[u8; 16], String> {
    let value = message
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("inbound message missing {key}"))?;
    let decoded = hex::decode(value).map_err(|err| format!("invalid {key} hex: {err}"))?;
    let decoded_len = decoded.len();
    decoded
        .try_into()
        .map_err(|_| format!("invalid {key} length {}, expected 16 bytes", decoded_len))
}

fn json_to_rmpv(value: &serde_json::Value) -> Result<rmpv::Value, String> {
    Ok(match value {
        serde_json::Value::Null => rmpv::Value::Nil,
        serde_json::Value::Bool(value) => rmpv::Value::Boolean(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_u64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_f64() {
                rmpv::Value::F64(value)
            } else {
                return Err("unsupported JSON number".to_string());
            }
        }
        serde_json::Value::String(value) => rmpv::Value::from(value.as_str()),
        serde_json::Value::Array(values) => {
            rmpv::Value::Array(values.iter().map(json_to_rmpv).collect::<Result<Vec<_>, _>>()?)
        }
        serde_json::Value::Object(map) => rmpv::Value::Map(
            map.iter()
                .map(|(key, value)| Ok((rmpv::Value::from(key.as_str()), json_to_rmpv(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}

fn load_effective_args(args: &Args) -> Result<EffectiveArgs, String> {
    let mut effective = EffectiveArgs {
        profile: "default".to_string(),
        rpc: DEFAULT_RPC_ADDR.to_string(),
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
        python_compat: PythonCompatConfig::default(),
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

fn prepare_lxmd_paths(config_arg: Option<&Path>) -> Result<LxmdPaths, String> {
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
    let generated_rnsconfig = config_dir.join(GENERATED_RETICULUMD_CONFIG);

    fs::create_dir_all(&messages_dir)
        .map_err(|err| format!("failed to create {}: {err}", messages_dir.display()))?;
    if !config_file.exists() {
        fs::write(&config_file, PYTHON_DEFAULT_LXMD_CONFIG)
            .map_err(|err| format!("failed to create {}: {err}", config_file.display()))?;
    }

    Ok(LxmdPaths {
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
    effective: &mut EffectiveArgs,
    paths: &LxmdPaths,
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

fn is_single_toml_config(path: &Path) -> Result<bool, String> {
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
    effective: &mut EffectiveArgs,
    config_path: &Path,
    paths: &LxmdPaths,
) -> Result<(), String> {
    let contents = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read {}: {err}", config_path.display()))?;
    let config: SingleTomlConfigFile = toml::from_str(&contents)
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
    interfaces: &[SingleTomlInterface],
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

fn parse_python_lxmd_config(
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

fn parse_python_reticulum_interfaces(input: &str) -> Vec<SingleTomlInterface> {
    #[derive(Default)]
    struct PythonIface {
        name: Option<String>,
        iface_type: Option<String>,
        enabled: Option<bool>,
        host: Option<String>,
        port: Option<u16>,
    }

    fn push_current(out: &mut Vec<SingleTomlInterface>, current: Option<PythonIface>) {
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
        out.push(SingleTomlInterface {
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

fn is_legacy_launcher_toml(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("toml")
}

fn apply_config_file(effective: &mut EffectiveArgs, config_path: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read {}: {err}", config_path.display()))?;
    let parsed: LxmdConfigFile = toml::from_str(&contents)
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

#[cfg(test)]
mod tests {
    use super::{
        apply_config_file, compact_json, compatibility_notes, decode_rpc_frame, encode_rpc_frame,
        is_single_toml_config, json_env, load_effective_args, parse_python_lxmd_config,
        parse_python_reticulum_interfaces, prepare_lxmd_paths, requires_supervised_launch,
        sanitize_file_name, EffectiveArgs,
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
        let encoded = encode_rpc_frame(value.clone()).expect("encode");
        let decoded = decode_rpc_frame(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn json_env_handles_strings_numbers_and_missing() {
        let value = json!({
            "id": "m1",
            "timestamp": 123,
        });
        assert_eq!(json_env(&value, "id"), "m1");
        assert_eq!(json_env(&value, "timestamp"), "123");
        assert_eq!(json_env(&value, "missing"), "");
    }

    #[test]
    fn compact_json_produces_single_line_json() {
        let value = json!({ "message": { "id": "m1" } });
        assert_eq!(compact_json(&value).expect("json"), "{\"message\":{\"id\":\"m1\"}}");
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

    #[test]
    fn sanitize_file_name_replaces_unsafe_characters() {
        assert_eq!(sanitize_file_name("msg:/id?"), "msg__id_");
    }
}
