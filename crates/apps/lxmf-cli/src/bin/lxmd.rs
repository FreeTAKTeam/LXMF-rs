use clap::Parser;
use config::load_effective_args;
use launch::{launch_supervised, requires_supervised_launch};
use python_compat::emit_compatibility_notes;
use serde_json::json;
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

#[path = "lxmd/config.rs"]
mod config;
#[path = "lxmd/config_python.rs"]
mod config_python;
#[path = "lxmd/inbound.rs"]
mod inbound;
#[path = "lxmd/launch.rs"]
mod launch;
#[path = "lxmd/python_compat.rs"]
mod python_compat;
#[path = "lxmd/query.rs"]
mod query;
#[path = "lxmd/rpc_client.rs"]
mod rpc_client;
#[path = "lxmd/types.rs"]
mod types;
#[path = "../version.rs"]
mod version;

pub(crate) use types::{
    EffectiveArgs, LxmdConfigFile, LxmdPaths, PythonCompatConfig, SingleTomlConfigFile,
    SingleTomlInterface,
};

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
#[command(
    name = "lxmd",
    about = "LXMF daemon compatibility entrypoint",
    disable_version_flag = true
)]
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

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = version::parse_with_version::<Args>();
    if args.exampleconfig {
        print!("{}", example_config());
        return ExitCode::SUCCESS;
    }

    let effective = match load_effective_args(&args) {
        Ok(effective) => effective,
        Err(err) => {
            log::error!("failed to load config: {err}");
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
    if let Some(config_dir) = effective.config_dir.as_ref() {
        cmd.arg("--rpc-unix").arg(config_dir.join("lxmf-rpc.sock"));
    }
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
        log::error!("failed to exec {}: {}", reticulumd.display(), err);
        ExitCode::from(1)
    }

    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
            Err(err) => {
                log::error!("failed to launch {}: {}", reticulumd.display(), err);
                ExitCode::from(1)
            }
        }
    }
}

fn maybe_handle_query_mode(args: &EffectiveArgs) -> Option<ExitCode> {
    if !(args.status || args.peers || args.sync.is_some() || args.unpeer.is_some()) {
        return None;
    }

    let rpc_addr = match query::resolve_query_rpc_addr(args.remote.as_deref(), &args.rpc) {
        Ok(rpc_addr) => rpc_addr,
        Err(err) => {
            log::error!("{err}");
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
                log::error!("{err}");
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
            log::error!("{err}");
            Some(ExitCode::from(1))
        }
    }
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
        apply_config_file, is_single_toml_config, load_effective_args, prepare_lxmd_paths,
    };
    use super::config_python::{parse_python_lxmd_config, parse_python_reticulum_interfaces};
    use super::python_compat::compatibility_notes;
    use super::{requires_supervised_launch, EffectiveArgs};
    use clap::Parser;
    use serde_json::json;
    use std::fs;

    #[test]
    fn compatibility_notes_only_emitted_for_used_flags() {
        let args = super::Args::parse_from(["lxmd", "--propagation-node", "--service"]);
        let effective = EffectiveArgs {
            profile: "default".into(),
            rpc: super::DEFAULT_RPC_ADDR.into(),
            rnsconfig: None,
            propagation_node: true,
            on_inbound: None,
            quiet: false,
            service: true,
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
        let notes = compatibility_notes(&args, &effective);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn propagation_node_uses_supervised_launch() {
        let effective = EffectiveArgs {
            profile: "default".into(),
            rpc: super::DEFAULT_RPC_ADDR.into(),
            rnsconfig: None,
            propagation_node: true,
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
    fn python_config_keeps_lxmf_peer_and_propagation_node_announce_intervals_separate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("lxmd");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        let config_path = config_dir.join("config");
        fs::write(
            &config_path,
            r#"
[lxmf]
announce_interval = 3

[propagation]
announce_interval = 2
"#,
        )
        .expect("write config");

        let args =
            super::Args::parse_from(["lxmd", "--config", config_path.to_str().expect("utf8 path")]);
        let effective = load_effective_args(&args).expect("effective args");

        assert_eq!(effective.python_compat.peer_announce_interval_min, Some(3));
        assert_eq!(effective.python_compat.node_announce_interval_min, Some(2));
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
