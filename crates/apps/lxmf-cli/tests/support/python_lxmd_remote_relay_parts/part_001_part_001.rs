use serde_json::{json, Value};

use std::fs::{self, File};

use std::io::{Read, Write};

use std::net::{TcpListener, TcpStream};

use std::path::{Path, PathBuf};

use std::process::{Child, Command, Stdio};

use std::thread;

use std::time::{Duration, Instant};

const TEST_TIMEOUT: Duration = Duration::from_secs(300);

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(500);

const RPC_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5);

const RPC_MAX_ATTEMPTS: usize = 60;

pub struct SpawnedNode {
    pub child: Child,
    stderr_log: PathBuf,
    rpc_port: u16,
}

pub struct SpawnedPythonRelay {
    pub child: Child,
    stderr_log: PathBuf,
}

pub struct SpawnedPythonEndpoint {
    pub child: Child,
    stderr_log: PathBuf,
    pub control_port: u16,
}

pub struct ReservedPort {
    listener: Option<TcpListener>,
    port: u16,
}

impl ReservedPort {
    pub fn reserve() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
        let port = listener.local_addr().expect("ephemeral local addr").port();
        Self { listener: Some(listener), port }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn release(&mut self) {
        self.listener.take();
    }
}

fn resolve_test_binary_if_present(name: &str, provided: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = provided.filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }

    if let Some(path) = std::env::var_os(format!("{}_BIN", name.to_ascii_uppercase()))
        .filter(|path| !path.is_empty())
    {
        return Some(PathBuf::from(path));
    }

    let current_exe = std::env::current_exe().expect("current test executable path");
    let deps_dir = current_exe.parent().expect("test executable parent");
    let target_dir = deps_dir.parent().expect("target debug dir");
    binary_candidates(target_dir, name).into_iter().find(|candidate| candidate.exists())
}

pub fn resolve_test_binary(name: &str, provided: Option<&str>) -> PathBuf {
    if let Some(path) = provided.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }

    if let Some(path) = std::env::var_os(format!("{}_BIN", name.to_ascii_uppercase()))
        .filter(|path| !path.is_empty())
    {
        return PathBuf::from(path);
    }

    build_workspace_binary(name).unwrap_or_else(|err| panic!("failed to build {name}: {err}"));
    if let Some(path) = resolve_test_binary_if_present(name, None) {
        return path;
    }

    panic!("failed to locate {name} test binary via CARGO_BIN_EXE or target/debug fallback");
}

fn build_workspace_binary(name: &str) -> Result<(), String> {
    let package = match name {
        "lxmd" => "lxmf-cli",
        "reticulumd" => "reticulumd",
        _ => return Err(format!("unknown workspace binary {name}")),
    };

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace root");
    let output = Command::new(cargo)
        .arg("build")
        .arg("-p")
        .arg(package)
        .arg("--bin")
        .arg(name)
        .current_dir(workspace_root)
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let mut details = Vec::new();
    if !stdout.is_empty() {
        details.push(format!("stdout:\n{stdout}"));
    }
    if !stderr.is_empty() {
        details.push(format!("stderr:\n{stderr}"));
    }
    if details.is_empty() {
        details.push(format!("exit status: {}", output.status));
    }
    Err(details.join("\n\n"))
}

fn binary_candidates(target_dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut candidates = vec![target_dir.join(name)];
    if !std::env::consts::EXE_SUFFIX.is_empty() {
        candidates.push(target_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)));
    }
    candidates
}

pub fn rust_node_config(
    name: &str,
    rpc_port: u16,
    transport_port: Option<u16>,
    interfaces: &[String],
) -> String {
    let interfaces = interfaces.join("\n");
    let transport = transport_port
        .map(|transport_port| format!("\n[transport]\nlisten = \"127.0.0.1:{transport_port}\"\n"))
        .unwrap_or_default();

    format!(
        r#"[node]
display_name = "{name}"

[rpc]
listen = "127.0.0.1:{rpc_port}"
{transport}

[storage]
db = "./state/reticulum.db"
identity = "./state/identity"

[lxmf]
announce_at_start = false

{interfaces}"#
    )
}

pub fn tcp_client_interface(name: &str, server_port: u16) -> String {
    format!(
        "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nname = \"{name}\"\nhost = \"127.0.0.1\"\nport = {server_port}\n"
    )
}

pub fn write_rust_config(dir: &Path, config: &str) {
    fs::create_dir_all(dir.join("state")).expect("create state dir");
    fs::write(dir.join("lxmd.toml"), config).expect("write rust config");
}

pub fn write_python_lxmd_config(dir: &Path, display_name: &str) {
    write_python_lxmd_config_with_propagation(dir, display_name, false);
}

pub fn write_python_lxmd_propagation_config(dir: &Path, display_name: &str) {
    write_python_lxmd_config_with_propagation(dir, display_name, true);
}

fn write_python_lxmd_config_with_propagation(
    dir: &Path,
    display_name: &str,
    propagation_enabled: bool,
) {
    fs::create_dir_all(dir).expect("create python lxmd dir");
    if !propagation_enabled {
        fs::write(
            dir.join("config"),
            format!(
                "[propagation]\nenable_node = no\nannounce_at_start = no\nautopeer = no\nauth_required = no\n\n[lxmf]\ndisplay_name = {display_name}\nannounce_at_start = no\ndelivery_transfer_max_accepted_size = 1000\n\n[logging]\nloglevel = 7\n"
            ),
        )
        .expect("write python lxmd config");
        return;
    }

    fs::write(
        dir.join("config"),
        format!(
            "[propagation]\nenable_node = yes\nannounce_at_start = yes\nannounce_interval = 1\nautopeer = yes\nautopeer_maxdepth = 6\nauth_required = no\npropagation_stamp_cost_target = 0\npropagation_stamp_cost_flexibility = 0\n\n[lxmf]\ndisplay_name = {display_name}\nannounce_at_start = no\ndelivery_transfer_max_accepted_size = 1000\n\n[logging]\nloglevel = 7\n"
        ),
    )
    .expect("write python lxmd config");
}

pub fn write_python_rns_config(dir: &Path, server_port: u16) {
    fs::create_dir_all(dir).expect("create python rns dir");
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = yes\nshare_instance = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[TCP Server Interface]]\n    type = TCPServerInterface\n    enabled = yes\n    listen_ip = 127.0.0.1\n    listen_port = {server_port}\n"
        ),
    )
    .expect("write python rns config");
}

pub fn write_python_client_rns_config(dir: &Path, server_port: u16) {
    fs::create_dir_all(dir).expect("create python client rns dir");
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[TCP Client Interface]]\n    type = TCPClientInterface\n    enabled = yes\n    target_host = 127.0.0.1\n    target_port = {server_port}\n"
        ),
    )
    .expect("write python client rns config");
}

pub fn spawn_lxmd(
    lxmd_bin: &Path,
    reticulumd_bin: &Path,
    rpc_port: u16,
    config_dir: &Path,
    reserved_ports: &mut [ReservedPort],
) -> SpawnedNode {
    for port in reserved_ports {
        port.release();
    }
    let stderr_log = config_dir.join("lxmd.stderr.log");
    let child = if live_child_logs_enabled() {
        log::info!("[live-logs] spawning rust {}", config_dir.display());
        Command::new(lxmd_bin)
            .arg("--config")
            .arg(config_dir.join("lxmd.toml"))
            .env("RETICULUMD_BIN", reticulumd_bin)
            .env("RETICULUMD_DIAGNOSTICS", "1")
            .env("RETICULUM_TRANSPORT_DIAGNOSTICS", "1")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn rust lxmd")
    } else {
        let stderr = File::create(&stderr_log).expect("create rust stderr log");
        Command::new(lxmd_bin)
            .arg("--config")
            .arg(config_dir.join("lxmd.toml"))
            .env("RETICULUMD_BIN", reticulumd_bin)
            .env("RETICULUMD_DIAGNOSTICS", "1")
            .env("RETICULUM_TRANSPORT_DIAGNOSTICS", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn rust lxmd")
    };
    SpawnedNode { child, stderr_log, rpc_port }
}

pub fn spawn_python_lxmd_relay(
    python_bin: &str,
    reticulum_repo: &str,
    lxmf_repo: &str,
    lxmd_dir: &Path,
    rns_dir: &Path,
    reserved_ports: &mut [ReservedPort],
) -> SpawnedPythonRelay {
    for port in reserved_ports {
        port.release();
    }
    let stderr_log = lxmd_dir.join("python-lxmd.stderr.log");
    let python_path = format!("{reticulum_repo}:{lxmf_repo}");
    let child = if live_child_logs_enabled() {
        log::info!("[live-logs] spawning python relay {}", lxmd_dir.display());
        Command::new(python_bin)
            .arg("-u")
            .arg("-m")
            .arg("LXMF.Utilities.lxmd")
            .arg("--config")
            .arg(lxmd_dir)
            .arg("--rnsconfig")
            .arg(rns_dir)
            .arg("-vv")
            .env("PYTHONPATH", python_path)
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn python lxmd relay")
    } else {
        let stderr = File::create(&stderr_log).expect("create python stderr log");
        Command::new(python_bin)
            .arg("-u")
            .arg("-m")
            .arg("LXMF.Utilities.lxmd")
            .arg("--config")
            .arg(lxmd_dir)
            .arg("--rnsconfig")
            .arg(rns_dir)
            .arg("-vv")
            .env("PYTHONPATH", python_path)
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn python lxmd relay")
    };
    SpawnedPythonRelay { child, stderr_log }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_python_endpoint(
    python_bin: &str,
    reticulum_repo: &str,
    lxmf_repo: &str,
    helper_script: &Path,
    node_name: &str,
    display_name: &str,
    rns_dir: &Path,
    storage_dir: &Path,
    control_port: u16,
    reserved_ports: &mut [ReservedPort],
) -> SpawnedPythonEndpoint {
    for port in reserved_ports {
        port.release();
    }
    fs::create_dir_all(storage_dir).expect("create python storage dir");
    let stderr_log = storage_dir.join(format!("{node_name}.stderr.log"));
    let python_path = format!("{reticulum_repo}:{lxmf_repo}");
    let child = if live_child_logs_enabled() {
        log::info!("[live-logs] spawning python endpoint {}", storage_dir.display());
        Command::new(python_bin)
            .arg("-u")
            .arg(helper_script)
            .arg("--name")
            .arg(node_name)
            .arg("--display-name")
            .arg(display_name)
            .arg("--rnsconfig")
            .arg(rns_dir)
            .arg("--storage")
            .arg(storage_dir)
            .arg("--control-port")
            .arg(control_port.to_string())
            .env("PYTHONPATH", python_path)
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn python endpoint")
    } else {
        let stderr = File::create(&stderr_log).expect("create python endpoint stderr log");
        Command::new(python_bin)
            .arg("-u")
            .arg(helper_script)
            .arg("--name")
            .arg(node_name)
            .arg("--display-name")
            .arg(display_name)
            .arg("--rnsconfig")
            .arg(rns_dir)
            .arg("--storage")
            .arg(storage_dir)
            .arg("--control-port")
            .arg(control_port.to_string())
            .env("PYTHONPATH", python_path)
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn python endpoint")
    };

    SpawnedPythonEndpoint { child, stderr_log, control_port }
}

pub fn wait_for_python_port(
    port: u16,
    relay: &mut SpawnedPythonRelay,
    label: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = relay.child.try_wait().map_err(|err| err.to_string())? {
            let stderr = read_log(relay.stderr_log.as_path());
            return Err(format!("{label} exited early with {status}: {stderr}"));
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            thread::sleep(Duration::from_secs(1));
            return Ok(());
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
    Err(format!("timed out waiting for {label} tcp listener on port {port}"))
}

pub fn wait_for_python_endpoint_ready(
    control_port: u16,
    endpoint: &mut SpawnedPythonEndpoint,
    label: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = endpoint.child.try_wait().map_err(|err| err.to_string())? {
            let stderr = read_log(endpoint.stderr_log.as_path());
            return Err(format!("{label} exited early with {status}: {stderr}"));
        }

        if python_control_call(control_port, "status", None).is_ok() {
            return Ok(());
        }

        thread::sleep(WAIT_POLL_INTERVAL);
    }

    Err(format!("timed out waiting for {label} control port {control_port}"))
}

pub fn wait_for_ready(rpc_port: u16, node: &mut SpawnedNode, label: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = node.child.try_wait().map_err(|err| err.to_string())? {
            let stderr = read_log(node.stderr_log.as_path());
            return Err(format!("{label} exited early with {status}: {stderr}"));
        }
        match http_get_ready(rpc_port) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => {}
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
    let stderr = read_log(node.stderr_log.as_path());
    if stderr.is_empty() {
        Err(format!("timed out waiting for {label} readyz on port {rpc_port}"))
    } else {
        Err(format!("timed out waiting for {label} readyz on port {rpc_port}; stderr: {stderr}"))
    }
}

pub fn daemon_status(rpc_port: u16) -> Result<Value, String> {
    rpc_call(rpc_port, "daemon_status_ex", None)
}
