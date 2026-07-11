pub fn status_hash(status: &Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) =
            status.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
        {
            return Some(hash.to_string());
        }
    }
    None
}

pub fn wait_for_known_path(rpc_port: u16, destination: &str) -> Result<(), String> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    let mut last_status = "path status was not queried".to_string();
    while Instant::now() < deadline {
        match rpc_call(
            rpc_port,
            "path_status",
            Some(json!({ "destination": destination })),
        ) {
            Ok(status) => {
                if status["known"].as_bool() == Some(true)
                    || status["path_found"].as_bool() == Some(true)
                {
                    return Ok(());
                }
                last_status = status.to_string();
            }
            Err(err) => last_status = format!("rpc error: {err}"),
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }

    Err(format!(
        "destination {destination} did not become reachable on rpc port {rpc_port}; last status: {last_status}"
    ))
}

pub fn wait_for_inbound_message(rpc_port: u16, expected_content: &str) -> Result<(), String> {
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

pub fn wait_for_python_inbound_message(
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

pub fn collect_node_diagnostics(
    label: &str,
    rpc_port: u16,
    node: Option<&mut SpawnedNode>,
) -> String {
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

pub fn collect_python_diagnostics(label: &str, relay: Option<&mut SpawnedPythonRelay>) -> String {
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

pub fn collect_python_endpoint_diagnostics(
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

pub fn python_control_call(
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

pub fn python_destination_hash(
    python_bin: &str,
    reticulum_repo: &str,
    lxmd_dir: &Path,
    destination_aspect: &str,
) -> Result<String, String> {
    let identity_path = lxmd_dir.join("identity");
    let python_path =
        std::env::join_paths([Path::new(reticulum_repo)]).map_err(|err| err.to_string())?;
    let script = r#"
import os
import sys
import tempfile

import RNS

identity_path, destination_aspect = sys.argv[1:3]
cfg = tempfile.mkdtemp(prefix="rns-hash-")
with open(os.path.join(cfg, "config"), "w", encoding="utf-8") as handle:
    handle.write(
        "[reticulum]\n"
        "share_instance = no\n"
        "enable_transport = no\n"
        "discover_interfaces = false\n"
        "autoconnect_discovered_interfaces = 0\n"
    )

RNS.Reticulum(configdir=cfg, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")

destination = RNS.Destination(
    identity,
    RNS.Destination.IN,
    RNS.Destination.SINGLE,
    "lxmf",
    destination_aspect,
)
print(RNS.hexrep(destination.hash, delimit=False).lower())
"#;
    let output = Command::new(python_bin)
        .arg("-c")
        .arg(script)
        .arg(&identity_path)
        .arg(destination_aspect)
        .env("PYTHONPATH", python_path)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(format!(
            "failed to derive Python destination hash from {}: {}{}{}",
            identity_path.display(),
            stderr,
            if stderr.is_empty() || stdout.is_empty() { "" } else { "\n" },
            stdout,
        ));
    }
    let hash = String::from_utf8(output.stdout).map_err(|err| err.to_string())?.trim().to_string();
    if hash.is_empty() {
        return Err(format!(
            "empty Python destination hash for {} aspect {}",
            identity_path.display(),
            destination_aspect
        ));
    }
    Ok(hash)
}

fn rpc_snapshot(rpc_port: u16, method: &str, params: Option<Value>) -> String {
    match rpc_call(rpc_port, method, params) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        Err(err) => format!("rpc error: {err}"),
    }
}

pub fn rpc_call(rpc_port: u16, method: &str, params: Option<Value>) -> Result<Value, String> {
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

pub fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }

    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(not(windows))]
    {
        let _ = child.kill();
    }

    let _ = child.wait();
}

fn live_child_logs_enabled() -> bool {
    std::env::var_os("LXMD_TEST_LOGS")
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

impl SpawnedNode {
    pub fn rpc_port(&self) -> u16 {
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
