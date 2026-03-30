use serde_json::{json, Value};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TEST_TIMEOUT: Duration = Duration::from_secs(300);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const REMOTE_PATH_RESPONSE_MIN: Duration = Duration::from_millis(900);
const RPC_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5);
const RPC_MAX_ATTEMPTS: usize = 60;

struct SpawnedNode {
    child: Child,
    stderr_log: PathBuf,
    rpc_port: u16,
}

struct SpawnedPythonRelay {
    child: Child,
    stderr_log: PathBuf,
}

struct SpawnedPythonEndpoint {
    child: Child,
    stderr_log: PathBuf,
    control_port: u16,
}

struct ReservedPort {
    listener: Option<TcpListener>,
    port: u16,
}

impl ReservedPort {
    fn reserve() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
        let port = listener.local_addr().expect("ephemeral local addr").port();
        Self { listener: Some(listener), port }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn release(&mut self) {
        self.listener.take();
    }
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn rust_to_python_lxmd_relay_remote_path_e2e() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));
    let workspace_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace root");

    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let reticulum_repo = env::var("RETICULUM_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("reticulum").display().to_string()
    });
    let lxmf_repo = env::var("LXMF_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("lxmf").display().to_string()
    });

    assert!(Path::new(&reticulum_repo).exists(), "reticulum repo not found: {reticulum_repo}");
    assert!(Path::new(&lxmf_repo).exists(), "lxmf repo not found: {lxmf_repo}");

    let temp = tempfile::tempdir().expect("tempdir");

    let upstream_server_port = ReservedPort::reserve();
    let upstream_server_port_num = upstream_server_port.port();
    let sender_rpc = ReservedPort::reserve();
    let sender_transport = ReservedPort::reserve();
    let rust_relay_rpc = ReservedPort::reserve();
    let rust_relay_transport = ReservedPort::reserve();
    let recipient_rpc = ReservedPort::reserve();
    let recipient_transport = ReservedPort::reserve();

    let python_lxmd_dir = temp.path().join("python-relay-lxmd");
    let python_rns_dir = temp.path().join("python-relay-rns");
    let sender_dir = temp.path().join("rust-sender");
    let rust_relay_dir = temp.path().join("rust-relay");
    let recipient_dir = temp.path().join("rust-recipient");

    write_python_lxmd_config(&python_lxmd_dir, "Python Relay");
    write_python_rns_config(&python_rns_dir, upstream_server_port_num);
    write_rust_config(
        &sender_dir,
        &rust_node_config(
            "rust-sender",
            sender_rpc.port(),
            Some(sender_transport.port()),
            &[tcp_client_interface("sender-uplink", upstream_server_port_num)],
        ),
    );
    write_rust_config(
        &rust_relay_dir,
        &rust_node_config(
            "rust-relay",
            rust_relay_rpc.port(),
            Some(rust_relay_transport.port()),
            &[tcp_client_interface("relay-uplink", upstream_server_port_num)],
        ),
    );
    write_rust_config(
        &recipient_dir,
        &rust_node_config(
            "rust-recipient",
            recipient_rpc.port(),
            Some(recipient_transport.port()),
            &[tcp_client_interface("recipient-uplink", rust_relay_transport.port())],
        ),
    );

    let mut python_relay = Some(spawn_python_lxmd_relay(
        &python_bin,
        &reticulum_repo,
        &lxmf_repo,
        &python_lxmd_dir,
        &python_rns_dir,
        &mut [upstream_server_port],
    ));
    let mut sender = None;
    let mut rust_relay = None;
    let mut recipient = None;

    let outcome: Result<(), String> = (|| {
        wait_for_python_port(
            upstream_server_port_num,
            python_relay.as_mut().expect("python relay child"),
            "python-relay",
        )?;

        rust_relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            rust_relay_rpc.port(),
            &rust_relay_dir,
            &mut [rust_relay_rpc, rust_relay_transport],
        ));
        wait_for_ready(
            rust_relay.as_ref().expect("rust relay child").rpc_port(),
            rust_relay.as_mut().expect("rust relay child"),
            "rust-relay",
        )?;

        recipient = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            recipient_rpc.port(),
            &recipient_dir,
            &mut [recipient_rpc, recipient_transport],
        ));
        wait_for_ready(
            recipient.as_ref().expect("recipient child").rpc_port(),
            recipient.as_mut().expect("recipient child"),
            "rust-recipient",
        )?;

        sender = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            sender_rpc.port(),
            &sender_dir,
            &mut [sender_rpc, sender_transport],
        ));
        wait_for_ready(
            sender.as_ref().expect("sender child").rpc_port(),
            sender.as_mut().expect("sender child"),
            "rust-sender",
        )?;

        let sender_rpc = sender.as_ref().expect("sender child").rpc_port();
        let recipient_rpc = recipient.as_ref().expect("recipient child").rpc_port();

        let recipient_status = daemon_status(recipient_rpc)?;
        let sender_status = daemon_status(sender_rpc)?;
        let recipient_hash = status_hash(&recipient_status)
            .unwrap_or_else(|| panic!("rust-recipient delivery hash: {recipient_status}"));
        let sender_hash = status_hash(&sender_status)
            .unwrap_or_else(|| panic!("rust-sender delivery hash: {sender_status}"));

        rpc_call(recipient_rpc, "announce_now", None)?;

        let message_id = format!(
            "python-relay-remote-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_millis()
        );
        let delivery_started_at = Instant::now();
        rpc_call(
            sender_rpc,
            "send_message_v2",
            Some(json!({
                "id": message_id,
                "source": sender_hash,
                "destination": recipient_hash,
                "title": "",
                "content": "hello through python relay",
                "method": "direct"
            })),
        )?;

        wait_for_inbound_message(recipient_rpc, "hello through python relay")?;

        let delivery_elapsed = delivery_started_at.elapsed();
        if delivery_elapsed < REMOTE_PATH_RESPONSE_MIN {
            return Err(format!(
                "python relay remote path response completed too quickly: {:?} < {:?}",
                delivery_elapsed, REMOTE_PATH_RESPONSE_MIN
            ));
        }

        Ok(())
    })();

    let sender_rpc = sender.as_ref().map_or(0, SpawnedNode::rpc_port);
    let rust_relay_rpc = rust_relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let recipient_rpc = recipient.as_ref().map_or(0, SpawnedNode::rpc_port);

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}\n\n{}",
            collect_python_diagnostics("python-relay", python_relay.as_mut()),
            collect_node_diagnostics("rust-sender", sender_rpc, sender.as_mut()),
            collect_node_diagnostics("rust-relay", rust_relay_rpc, rust_relay.as_mut()),
            collect_node_diagnostics("rust-recipient", recipient_rpc, recipient.as_mut()),
        ))
    } else {
        None
    };

    if let Some(node) = sender.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = recipient.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = rust_relay.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_relay.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("python lxmd remote relay flow failed:\n{details}");
    }
}

#[test]
#[ignore = "requires local Python Reticulum/LXMF repos and daemon runtime"]
fn python_to_rust_lxmd_relay_remote_path_e2e() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));
    let workspace_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).expect("workspace root");
    let helper_script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("python_lxmf_endpoint.py");

    let python_bin = env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let reticulum_repo = env::var("RETICULUM_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("reticulum").display().to_string()
    });
    let lxmf_repo = env::var("LXMF_PY_REPO").unwrap_or_else(|_| {
        workspace_root.parent().expect("workspace parent").join("lxmf").display().to_string()
    });

    assert!(Path::new(&reticulum_repo).exists(), "reticulum repo not found: {reticulum_repo}");
    assert!(Path::new(&lxmf_repo).exists(), "lxmf repo not found: {lxmf_repo}");
    assert!(helper_script.exists(), "python helper script not found: {}", helper_script.display());

    let temp = tempfile::tempdir().expect("tempdir");

    let upstream_relay_rpc = ReservedPort::reserve();
    let upstream_relay_transport = ReservedPort::reserve();
    let downstream_relay_rpc = ReservedPort::reserve();
    let downstream_relay_transport = ReservedPort::reserve();
    let python_sender_control = ReservedPort::reserve();
    let python_recipient_control = ReservedPort::reserve();

    let upstream_relay_dir = temp.path().join("rust-upstream-relay");
    let downstream_relay_dir = temp.path().join("rust-downstream-relay");
    let python_sender_storage = temp.path().join("python-sender-storage");
    let python_sender_rns = temp.path().join("python-sender-rns");
    let python_recipient_storage = temp.path().join("python-recipient-storage");
    let python_recipient_rns = temp.path().join("python-recipient-rns");

    write_rust_config(
        &upstream_relay_dir,
        &rust_node_config(
            "rust-upstream-relay",
            upstream_relay_rpc.port(),
            Some(upstream_relay_transport.port()),
            &[],
        ),
    );
    write_rust_config(
        &downstream_relay_dir,
        &rust_node_config(
            "rust-downstream-relay",
            downstream_relay_rpc.port(),
            Some(downstream_relay_transport.port()),
            &[tcp_client_interface("downstream-uplink", upstream_relay_transport.port())],
        ),
    );
    write_python_client_rns_config(&python_sender_rns, upstream_relay_transport.port());
    write_python_client_rns_config(&python_recipient_rns, downstream_relay_transport.port());

    let mut upstream_relay = None;
    let mut downstream_relay = None;
    let mut python_sender = None;
    let mut python_recipient = None;

    let outcome: Result<(), String> = (|| {
        upstream_relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            upstream_relay_rpc.port(),
            &upstream_relay_dir,
            &mut [upstream_relay_rpc, upstream_relay_transport],
        ));
        wait_for_ready(
            upstream_relay.as_ref().expect("upstream relay child").rpc_port(),
            upstream_relay.as_mut().expect("upstream relay child"),
            "rust-upstream-relay",
        )?;

        downstream_relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            downstream_relay_rpc.port(),
            &downstream_relay_dir,
            &mut [downstream_relay_rpc, downstream_relay_transport],
        ));
        wait_for_ready(
            downstream_relay.as_ref().expect("downstream relay child").rpc_port(),
            downstream_relay.as_mut().expect("downstream relay child"),
            "rust-downstream-relay",
        )?;

        python_recipient = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-recipient",
            "Python Recipient",
            &python_recipient_rns,
            &python_recipient_storage,
            python_recipient_control.port(),
            &mut [python_recipient_control],
        ));
        wait_for_python_endpoint_ready(
            python_recipient.as_ref().expect("python recipient").control_port,
            python_recipient.as_mut().expect("python recipient"),
            "python-recipient",
        )?;

        python_sender = Some(spawn_python_endpoint(
            &python_bin,
            &reticulum_repo,
            &lxmf_repo,
            &helper_script,
            "python-sender",
            "Python Sender",
            &python_sender_rns,
            &python_sender_storage,
            python_sender_control.port(),
            &mut [python_sender_control],
        ));
        wait_for_python_endpoint_ready(
            python_sender.as_ref().expect("python sender").control_port,
            python_sender.as_mut().expect("python sender"),
            "python-sender",
        )?;

        let sender_control = python_sender.as_ref().expect("python sender").control_port;
        let recipient_control = python_recipient.as_ref().expect("python recipient").control_port;

        let recipient_status = python_control_call(recipient_control, "status", None)?;
        let recipient_hash = recipient_status
            .get("delivery_destination_hash")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("missing python recipient delivery hash: {recipient_status}"))?;

        python_control_call(recipient_control, "announce", None)?;

        let delivery_started_at = Instant::now();
        python_control_call(
            sender_control,
            "send_message",
            Some(json!({
                "destination": recipient_hash,
                "title": "",
                "content": "hello through rust relay",
            })),
        )?;

        wait_for_python_inbound_message(recipient_control, "hello through rust relay")?;

        let delivery_elapsed = delivery_started_at.elapsed();
        if delivery_elapsed < REMOTE_PATH_RESPONSE_MIN {
            return Err(format!(
                "rust relay remote path response completed too quickly: {:?} < {:?}",
                delivery_elapsed, REMOTE_PATH_RESPONSE_MIN
            ));
        }

        Ok(())
    })();

    let upstream_relay_rpc = upstream_relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let downstream_relay_rpc = downstream_relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let python_sender_control = python_sender.as_ref().map_or(0, |node| node.control_port);
    let python_recipient_control = python_recipient.as_ref().map_or(0, |node| node.control_port);

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}\n\n{}",
            collect_node_diagnostics(
                "rust-upstream-relay",
                upstream_relay_rpc,
                upstream_relay.as_mut()
            ),
            collect_node_diagnostics(
                "rust-downstream-relay",
                downstream_relay_rpc,
                downstream_relay.as_mut()
            ),
            collect_python_endpoint_diagnostics(
                "python-sender",
                python_sender_control,
                python_sender.as_mut(),
            ),
            collect_python_endpoint_diagnostics(
                "python-recipient",
                python_recipient_control,
                python_recipient.as_mut(),
            ),
        ))
    } else {
        None
    };

    if let Some(node) = python_sender.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = python_recipient.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = downstream_relay.as_mut() {
        terminate_child(&mut node.child);
    }
    if let Some(node) = upstream_relay.as_mut() {
        terminate_child(&mut node.child);
    }

    if let Some(details) = failure_details {
        panic!("python to rust lxmd remote relay flow failed:\n{details}");
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

fn resolve_test_binary(name: &str, provided: Option<&str>) -> PathBuf {
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

fn rust_node_config(
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

fn tcp_client_interface(name: &str, server_port: u16) -> String {
    format!(
        "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nname = \"{name}\"\nhost = \"127.0.0.1\"\nport = {server_port}\n"
    )
}

fn write_rust_config(dir: &Path, config: &str) {
    fs::create_dir_all(dir.join("state")).expect("create state dir");
    fs::write(dir.join("lxmd.toml"), config).expect("write rust config");
}

fn write_python_lxmd_config(dir: &Path, display_name: &str) {
    fs::create_dir_all(dir).expect("create python lxmd dir");
    fs::write(
        dir.join("config"),
        format!(
            "[propagation]\nenable_node = no\nannounce_at_start = no\nautopeer = no\nauth_required = no\n\n[lxmf]\ndisplay_name = {display_name}\nannounce_at_start = no\ndelivery_transfer_max_accepted_size = 1000\n\n[logging]\nloglevel = 7\n"
        ),
    )
    .expect("write python lxmd config");
}

fn write_python_rns_config(dir: &Path, server_port: u16) {
    fs::create_dir_all(dir).expect("create python rns dir");
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = yes\nshare_instance = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[TCP Server Interface]]\n    type = TCPServerInterface\n    enabled = yes\n    listen_ip = 127.0.0.1\n    listen_port = {server_port}\n"
        ),
    )
    .expect("write python rns config");
}

fn write_python_client_rns_config(dir: &Path, server_port: u16) {
    fs::create_dir_all(dir).expect("create python client rns dir");
    fs::write(
        dir.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 7\n\n[interfaces]\n  [[TCP Client Interface]]\n    type = TCPClientInterface\n    enabled = yes\n    target_host = 127.0.0.1\n    target_port = {server_port}\n"
        ),
    )
    .expect("write python client rns config");
}

fn spawn_lxmd(
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
        eprintln!("[live-logs] spawning rust {}", config_dir.display());
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

fn spawn_python_lxmd_relay(
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
        eprintln!("[live-logs] spawning python relay {}", lxmd_dir.display());
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
fn spawn_python_endpoint(
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
        eprintln!("[live-logs] spawning python endpoint {}", storage_dir.display());
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

fn wait_for_python_port(
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

fn wait_for_python_endpoint_ready(
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

fn wait_for_ready(rpc_port: u16, node: &mut SpawnedNode, label: &str) -> Result<(), String> {
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

fn daemon_status(rpc_port: u16) -> Result<Value, String> {
    rpc_call(rpc_port, "daemon_status_ex", None)
}

fn status_hash(status: &Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) =
            status.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
        {
            return Some(hash.to_string());
        }
    }
    None
}

fn wait_for_inbound_message(rpc_port: u16, expected_content: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        let messages = rpc_call(rpc_port, "list_messages", None)?;
        let delivered = messages["messages"].as_array().is_some_and(|items| {
            items.iter().any(|message| {
                message["direction"].as_str() == Some("in")
                    && message["content"].as_str() == Some(expected_content)
            })
        });
        if delivered {
            return Ok(());
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
    Err(format!("inbound content '{expected_content}' did not arrive on rpc port {rpc_port}"))
}

fn wait_for_python_inbound_message(
    control_port: u16,
    expected_content: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        let messages = python_control_call(control_port, "list_messages", None)?;
        let delivered = messages["messages"].as_array().is_some_and(|items| {
            items.iter().any(|message| message["content"].as_str() == Some(expected_content))
        });
        if delivered {
            return Ok(());
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }

    Err(format!(
        "python inbound content '{expected_content}' did not arrive on control port {control_port}"
    ))
}

fn collect_node_diagnostics(label: &str, rpc_port: u16, node: Option<&mut SpawnedNode>) -> String {
    let Some(node) = node else {
        return format!("{label} diagnostics:\nnode was not started");
    };

    let process_state = match node.child.try_wait() {
        Ok(Some(status)) => format!("exited: {status}"),
        Ok(None) => "running".to_string(),
        Err(err) => format!("status error: {err}"),
    };

    let status = rpc_snapshot(rpc_port, "daemon_status_ex", None);
    let peers = rpc_snapshot(rpc_port, "list_peers", None);
    let announces = rpc_snapshot(rpc_port, "list_announces", Some(json!({ "limit": 50 })));
    let messages = rpc_snapshot(rpc_port, "list_messages", None);
    let interfaces = rpc_snapshot(rpc_port, "list_interfaces", None);
    let stderr = trim_log(read_log(node.stderr_log.as_path()), 16_000);

    format!(
        "{label} diagnostics:\nprocess: {process_state}\nrpc_port: {rpc_port}\ndaemon_status_ex: {status}\nlist_peers: {peers}\nlist_announces: {announces}\nlist_messages: {messages}\nlist_interfaces: {interfaces}\nstderr:\n{stderr}"
    )
}

fn collect_python_diagnostics(label: &str, relay: Option<&mut SpawnedPythonRelay>) -> String {
    let Some(relay) = relay else {
        return format!("{label} diagnostics:\nnode was not started");
    };

    let process_state = match relay.child.try_wait() {
        Ok(Some(status)) => format!("exited: {status}"),
        Ok(None) => "running".to_string(),
        Err(err) => format!("status error: {err}"),
    };
    let stderr = trim_log(read_log(relay.stderr_log.as_path()), 16_000);
    format!("{label} diagnostics:\nprocess: {process_state}\nstderr:\n{stderr}")
}

fn collect_python_endpoint_diagnostics(
    label: &str,
    control_port: u16,
    endpoint: Option<&mut SpawnedPythonEndpoint>,
) -> String {
    let Some(endpoint) = endpoint else {
        return format!("{label} diagnostics:\nnode was not started");
    };

    let process_state = match endpoint.child.try_wait() {
        Ok(Some(status)) => format!("exited: {status}"),
        Ok(None) => "running".to_string(),
        Err(err) => format!("status error: {err}"),
    };
    let status = python_control_snapshot(control_port, "status", None);
    let messages = python_control_snapshot(control_port, "list_messages", None);
    let stderr = trim_log(read_log(endpoint.stderr_log.as_path()), 16_000);
    format!(
        "{label} diagnostics:\nprocess: {process_state}\ncontrol_port: {control_port}\nstatus: {status}\nlist_messages: {messages}\nstderr:\n{stderr}"
    )
}

fn python_control_snapshot(control_port: u16, method: &str, params: Option<Value>) -> String {
    match python_control_call(control_port, method, params) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(err) => format!("control error: {err}"),
    }
}

fn python_control_call(
    control_port: u16,
    method: &str,
    params: Option<Value>,
) -> Result<Value, String> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", control_port)).map_err(|err| err.to_string())?;
    let request = json!({
        "method": method,
        "params": params.unwrap_or(Value::Null),
    });
    let mut bytes = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|err| err.to_string())?;
    let text = String::from_utf8(response).map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_str(text.trim()).map_err(|err| err.to_string())?;
    if !value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown python control error")
            .to_string());
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

fn rpc_snapshot(rpc_port: u16, method: &str, params: Option<Value>) -> String {
    match rpc_call(rpc_port, method, params) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(err) => format!("rpc error: {err}"),
    }
}

fn rpc_call(rpc_port: u16, method: &str, params: Option<Value>) -> Result<Value, String> {
    for attempt in 0..RPC_MAX_ATTEMPTS {
        let payload = encode_rpc_frame(json!({
            "id": 1,
            "method": method,
            "params": params.clone(),
        }))?;
        let request = format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{rpc_port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let mut bytes = request.into_bytes();
        bytes.extend_from_slice(&payload);
        let response = http_request(rpc_port, &bytes)?;
        let body = http_body(&response).ok_or_else(|| "missing rpc response body".to_string())?;
        let value: Value = decode_rpc_frame(body)?;
        if let Some(items) = value.as_array() {
            if rpc_error_is_rate_limited(&value) && attempt + 1 < RPC_MAX_ATTEMPTS {
                thread::sleep(RPC_RATE_LIMIT_BACKOFF);
                continue;
            }
            if rpc_value_is_direct_error(items) {
                return Err(value.to_string());
            }
            let result = items.get(1).cloned().unwrap_or(Value::Null);
            let error = items.get(2).cloned().unwrap_or(Value::Null);
            if !error.is_null() {
                if rpc_error_is_rate_limited(&error) && attempt + 1 < RPC_MAX_ATTEMPTS {
                    thread::sleep(RPC_RATE_LIMIT_BACKOFF);
                    continue;
                }
                return Err(error.to_string());
            }
            return Ok(result);
        }

        let result = value.get("result").unwrap_or(&value);
        if let Some(error) = value.get("error").or_else(|| result.get("error")) {
            if !error.is_null() {
                if rpc_error_is_rate_limited(error) && attempt + 1 < RPC_MAX_ATTEMPTS {
                    thread::sleep(RPC_RATE_LIMIT_BACKOFF);
                    continue;
                }
                return Err(error.to_string());
            }
        }
        return Ok(result.clone());
    }

    Err(format!("rpc call {method} exhausted retry budget"))
}

fn rpc_error_is_rate_limited(error: &Value) -> bool {
    error.as_str() == Some("SDK_SECURITY_RATE_LIMITED")
        || error.as_array().and_then(|items| items.first()).and_then(Value::as_str)
            == Some("SDK_SECURITY_RATE_LIMITED")
        || error.get("code").and_then(Value::as_str) == Some("SDK_SECURITY_RATE_LIMITED")
}

fn rpc_value_is_direct_error(items: &[Value]) -> bool {
    items.first().and_then(Value::as_str).is_some_and(|code| code.starts_with("SDK_"))
}

fn http_get_ready(rpc_port: u16) -> Result<bool, String> {
    let response = http_request(
        rpc_port,
        format!("GET /readyz HTTP/1.1\r\nHost: 127.0.0.1:{rpc_port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )?;
    Ok(response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200"))
}

fn http_request(rpc_port: u16, request: &[u8]) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", rpc_port)).map_err(|err| err.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|err| err.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|err| err.to_string())?;
    stream.write_all(request).map_err(|err| err.to_string())?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|err| err.to_string())?;
    Ok(response)
}

fn http_body(response: &[u8]) -> Option<&[u8]> {
    response.windows(4).position(|window| window == b"\r\n\r\n").map(|index| &response[index + 4..])
}

fn encode_rpc_frame(value: Value) -> Result<Vec<u8>, String> {
    let payload = rmp_serde::to_vec(&value).map_err(|err| err.to_string())?;
    let len = u32::try_from(payload.len()).map_err(|_| "rpc frame too large".to_string())?;
    let mut bytes = len.to_be_bytes().to_vec();
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_rpc_frame(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() < 4 {
        return Err("rpc response too short".to_string());
    }
    let frame_len = u32::from_be_bytes(bytes[..4].try_into().expect("frame header")) as usize;
    if bytes.len() < 4 + frame_len {
        return Err("rpc response incomplete".to_string());
    }
    rmp_serde::from_slice(&bytes[4..4 + frame_len]).map_err(|err| err.to_string())
}

fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn live_child_logs_enabled() -> bool {
    std::env::var_os("LXMD_TEST_LOGS")
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

impl SpawnedNode {
    fn rpc_port(&self) -> u16 {
        self.rpc_port
    }
}

fn read_log(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn trim_log(mut text: String, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text;
    }

    let split_at = text.len().saturating_sub(max_chars);
    text.drain(..split_at);
    format!("...<truncated>\n{text}")
}
