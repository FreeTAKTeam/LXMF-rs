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
