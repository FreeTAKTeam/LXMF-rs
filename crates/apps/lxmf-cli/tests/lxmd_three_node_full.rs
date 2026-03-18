use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TEST_TIMEOUT: Duration = Duration::from_secs(300);
const RPC_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5);
const RPC_MAX_ATTEMPTS: usize = 60;

struct SpawnedNode {
    child: Child,
    stderr_log: PathBuf,
}

#[test]
fn lxmd_three_node_tcp_full_test_waits_for_announces_before_delivery() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));

    let temp = tempfile::tempdir().expect("tempdir");
    let server_port = reserve_local_port();
    let server_rpc = reserve_local_port();
    let client_two_rpc = reserve_local_port();
    let client_two_transport = reserve_local_port();
    let client_three_rpc = reserve_local_port();
    let client_three_transport = reserve_local_port();

    let server_dir = temp.path().join("server");
    let client_two_dir = temp.path().join("client-two");
    let client_three_dir = temp.path().join("client-three");

    write_config(
        &server_dir,
        &node_config("server", server_rpc, server_port, "tcp_server", server_port),
    );
    write_config(
        &client_two_dir,
        &node_config("client-two", client_two_rpc, client_two_transport, "tcp_client", server_port),
    );
    write_config(
        &client_three_dir,
        &node_config(
            "client-three",
            client_three_rpc,
            client_three_transport,
            "tcp_client",
            server_port,
        ),
    );

    let mut server = Some(spawn_lxmd(&lxmd_bin, &reticulumd_bin, &server_dir));
    let mut client_two = None;
    let mut client_three = None;

    let outcome: Result<(), String> = (|| {
        wait_for_ready(server_rpc, server.as_mut().expect("server child"), "server")?;

        client_two = Some(spawn_lxmd(&lxmd_bin, &reticulumd_bin, &client_two_dir));
        wait_for_ready(
            client_two_rpc,
            client_two.as_mut().expect("client-two child"),
            "client-two",
        )?;

        client_three = Some(spawn_lxmd(&lxmd_bin, &reticulumd_bin, &client_three_dir));
        wait_for_ready(
            client_three_rpc,
            client_three.as_mut().expect("client-three child"),
            "client-three",
        )?;

        let client_two_status = daemon_status(client_two_rpc)?;
        let client_three_status = daemon_status(client_three_rpc)?;
        let client_two_hash = status_hash(&client_two_status)
            .unwrap_or_else(|| panic!("client-two delivery hash: {client_two_status}"));
        let client_three_hash = status_hash(&client_three_status)
            .unwrap_or_else(|| panic!("client-three delivery hash: {client_three_status}"));

        rpc_call(client_two_rpc, "announce_now", None)?;
        rpc_call(client_three_rpc, "announce_now", None)?;

        wait_for_peer(client_two_rpc, &client_three_hash)?;
        wait_for_peer(client_three_rpc, &client_two_hash)?;

        let message_id = format!(
            "hello-world-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_millis()
        );
        rpc_call(
            client_three_rpc,
            "send_message_v2",
            Some(json!({
                "id": message_id,
                "source": client_three_hash,
                "destination": client_two_hash,
                "title": "",
                "content": "hello world",
                "method": "direct"
            })),
        )?;

        wait_for_inbound_message(client_two_rpc, "hello world")
    })();

    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}",
            collect_node_diagnostics("server", server_rpc, server.as_mut()),
            collect_node_diagnostics("client-two", client_two_rpc, client_two.as_mut()),
            collect_node_diagnostics("client-three", client_three_rpc, client_three.as_mut()),
        ))
    } else {
        None
    };

    if let Some(node) = client_three.as_mut() {
        terminate_child(node);
    }
    if let Some(node) = client_two.as_mut() {
        terminate_child(node);
    }
    if let Some(node) = server.as_mut() {
        terminate_child(node);
    }

    if let Some(details) = failure_details {
        panic!("three-node lxmd flow failed:\n{details}");
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
    if let Some(path) = resolve_test_binary_if_present(name, provided) {
        return path;
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

fn node_config(
    name: &str,
    rpc_port: u16,
    transport_port: u16,
    interface_type: &str,
    server_port: u16,
) -> String {
    let interface = match interface_type {
        "tcp_server" => String::new(),
        "tcp_client" => format!(
            "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nname = \"{name}-uplink\"\nhost = \"127.0.0.1\"\nport = {server_port}\n"
        ),
        other => panic!("unsupported interface type {other}"),
    };

    format!(
        r#"[node]
display_name = "{name}"

[rpc]
listen = "127.0.0.1:{rpc_port}"

[transport]
listen = "127.0.0.1:{transport_port}"

[storage]
db = "./state/reticulum.db"
identity = "./state/identity"

[lxmf]
announce_at_start = false

{interface}"#
    )
}

fn write_config(dir: &Path, config: &str) {
    fs::create_dir_all(dir.join("state")).expect("create state dir");
    fs::write(dir.join("lxmd.toml"), config).expect("write config");
}

fn spawn_lxmd(lxmd_bin: &Path, reticulumd_bin: &Path, config_dir: &Path) -> SpawnedNode {
    let stderr_log = config_dir.join("lxmd.stderr.log");
    let stderr = File::create(&stderr_log).expect("create stderr log");
    let child = Command::new(lxmd_bin)
        .arg("--config")
        .arg(config_dir.join("lxmd.toml"))
        .env("RETICULUMD_BIN", reticulumd_bin)
        .env("RETICULUMD_DIAGNOSTICS", "1")
        .env("RETICULUM_TRANSPORT_DIAGNOSTICS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn lxmd");
    SpawnedNode { child, stderr_log }
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
        thread::sleep(Duration::from_millis(200));
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

fn wait_for_peer(rpc_port: u16, expected_peer: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while Instant::now() < deadline {
        let peers = rpc_call(rpc_port, "list_peers", None)?;
        let seen = peers["peers"].as_array().is_some_and(|items| {
            items.iter().any(|peer| {
                peer["peer"].as_str() == Some(expected_peer)
                    || peer["destination"].as_str() == Some(expected_peer)
                    || peer["source"].as_str() == Some(expected_peer)
            })
        });
        if seen {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(2));
    }
    Err(format!("peer {expected_peer} was not discovered on rpc port {rpc_port}"))
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
        thread::sleep(Duration::from_secs(2));
    }
    Err(format!("inbound content '{expected_content}' did not arrive on rpc port {rpc_port}"))
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

fn terminate_child(node: &mut SpawnedNode) {
    if node.child.try_wait().ok().flatten().is_none() {
        let _ = node.child.kill();
        let _ = node.child.wait();
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

fn reserve_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("ephemeral local addr")
        .port()
}
