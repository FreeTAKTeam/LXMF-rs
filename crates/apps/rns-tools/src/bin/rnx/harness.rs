use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rns_rpc::e2e_harness::{
    build_daemon_args, build_http_post, build_rpc_frame, is_ready_line, parse_http_response_body,
    parse_rpc_frame, peer_present,
};

const RPC_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5);
const RPC_MAX_ATTEMPTS: usize = 60;

pub(crate) fn ensure_rpc_ok(
    response: rns_rpc::RpcResponse,
    context: &str,
) -> io::Result<Option<serde_json::Value>> {
    if let Some(error) = response.error {
        return Err(io::Error::other(format!(
            "{} failed: {} ({})",
            context, error.message, error.code
        )));
    }
    Ok(response.result)
}

pub(crate) fn spawn_daemon(
    rpc: &str,
    db_path: &Path,
    transport: &str,
    config: &Path,
    propagation_enabled: bool,
) -> io::Result<Child> {
    spawn_daemon_with_optional_transport(
        rpc,
        db_path,
        Some(transport),
        config,
        propagation_enabled,
        false,
    )
}

pub(crate) fn spawn_daemon_with_optional_transport(
    rpc: &str,
    db_path: &Path,
    transport: Option<&str>,
    config: &Path,
    propagation_enabled: bool,
    diagnostics: bool,
) -> io::Result<Child> {
    let mut cmd = ProcessCommand::new(reticulumd_path()?);
    cmd.args(build_daemon_args(
        rpc,
        &db_path.to_string_lossy(),
        0,
        transport,
        Some(&config.to_string_lossy()),
    ));
    if propagation_enabled {
        cmd.env("LXMD_PROPAGATION_NODE", "1");
    }
    if diagnostics {
        cmd.env("RUST_LOG", "reticulumd=trace,reticulum_rs_transport=trace");
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(if diagnostics || stderr_passthrough_enabled() {
        Stdio::inherit()
    } else {
        Stdio::null()
    });
    cmd.spawn()
}

fn stderr_passthrough_enabled() -> bool {
    std::env::var("RNX_DAEMON_STDERR")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "debug"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn derive_preferred_transport_port(rpc_port: u16, offset: u16) -> io::Result<u16> {
    rpc_port.checked_add(offset).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "transport port overflow derived from rpc port")
    })
}

pub(crate) fn reserve_port(preferred: u16, reserved: &HashSet<u16>) -> io::Result<TcpListener> {
    if !reserved.contains(&preferred) {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", preferred)) {
            return Ok(listener);
        }
    }

    for _ in 0..16 {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        if !reserved.contains(&port) {
            return Ok(listener);
        }
    }

    Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "failed to reserve a network port"))
}

fn reticulumd_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::other("missing exe parent"))?;
    let candidate = dir.join("reticulumd");
    if candidate.exists() {
        Ok(candidate)
    } else {
        Ok(PathBuf::from("reticulumd"))
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DaemonReady {
    pub(crate) delivery_hash: Option<String>,
    pub(crate) propagation_hash: Option<String>,
}

pub(crate) fn wait_for_ready<R: Read + Send + 'static>(
    reader: R,
    timeout: Duration,
) -> io::Result<DaemonReady> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(reader).lines();
        while let Some(Ok(line)) = lines.next() {
            let _ = tx.send(line);
        }
    });

    let deadline = Instant::now() + timeout;
    let mut ready = DaemonReady::default();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon did not become ready"));
        }
        let remaining = deadline.saturating_duration_since(now);
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                ready.delivery_hash =
                    ready.delivery_hash.or_else(|| parse_delivery_destination_hash(&line));
                ready.propagation_hash =
                    ready.propagation_hash.or_else(|| parse_propagation_destination_hash(&line));
                if is_ready_line(&line) {
                    return Ok(ready);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "daemon stdout closed"));
            }
        }
    }
}

pub(crate) fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<rns_rpc::RpcResponse> {
    for attempt in 0..RPC_MAX_ATTEMPTS {
        let frame = build_rpc_frame(id, method, params.clone())?;
        let request = build_http_post("/rpc", rpc, &frame);
        let mut stream = TcpStream::connect(rpc)?;
        stream.write_all(&request)?;
        stream.shutdown(Shutdown::Write)?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        let body = parse_http_response_body(&response)?;
        let parsed = parse_rpc_frame(&body).map_err(|error| {
            let body_hex = hex::encode(body.iter().copied().take(64).collect::<Vec<_>>());
            io::Error::new(
                error.kind(),
                format!(
                    "rpc call {method} decode failed: {error}; body_len={} body_prefix_hex={}",
                    body.len(),
                    body_hex
                ),
            )
        })?;
        if rpc_response_is_rate_limited(&parsed) && attempt + 1 < RPC_MAX_ATTEMPTS {
            std::thread::sleep(RPC_RATE_LIMIT_BACKOFF);
            continue;
        }
        return Ok(parsed);
    }

    Err(io::Error::other(format!(
        "rpc call {method} exhausted retry budget after repeated rate limiting"
    )))
}

fn rpc_response_is_rate_limited(response: &rns_rpc::RpcResponse) -> bool {
    response.error.as_ref().is_some_and(|error| error.code == "SDK_SECURITY_RATE_LIMITED")
}

pub(crate) fn poll_for_inbound_content(
    rpc: &str,
    expected_content: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let response = rpc_call(rpc, request_id, "list_messages", None)?;
        request_id = request_id.wrapping_add(1);
        if inbound_content_present(&response, expected_content) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(crate) fn poll_for_any_peer(
    rpc: &str,
    timeout: Duration,
    mut request_id: u64,
    exclude_peer: Option<&str>,
) -> io::Result<Option<String>> {
    let deadline = Instant::now() + timeout;
    loop {
        let response = rpc_call(rpc, request_id, "list_peers", None)?;
        request_id = request_id.wrapping_add(1);
        if let Some(peer) = first_peer(&response, exclude_peer) {
            return Ok(Some(peer));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(crate) fn poll_for_peer(
    rpc: &str,
    expected_peer: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let response = rpc_call(rpc, request_id, "list_peers", None)?;
        request_id = request_id.wrapping_add(1);
        if peer_present(&response, expected_peer) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn first_peer(response: &rns_rpc::RpcResponse, exclude_peer: Option<&str>) -> Option<String> {
    let result = response.result.as_ref()?;
    let peers = result.get("peers")?.as_array()?;
    peers.iter().find_map(|entry| {
        let candidate = entry.get("peer").and_then(|value| value.as_str())?;
        if Some(candidate) == exclude_peer {
            None
        } else {
            Some(candidate.to_owned())
        }
    })
}

fn parse_delivery_destination_hash(line: &str) -> Option<String> {
    let marker = "delivery destination hash=";
    let idx = line.find(marker)?;
    let start = idx + marker.len();
    let value = line[start..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn parse_propagation_destination_hash(line: &str) -> Option<String> {
    let marker = "propagation destination hash=";
    let idx = line.find(marker)?;
    let start = idx + marker.len();
    let value = line[start..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn inbound_content_present(response: &rns_rpc::RpcResponse, expected_content: &str) -> bool {
    let Some(result) = response.result.as_ref() else {
        return false;
    };
    let Some(messages) = result.get("messages").and_then(|value| value.as_array()) else {
        return false;
    };
    messages.iter().any(|message| {
        message.get("direction").and_then(|value| value.as_str()) == Some("in")
            && message.get("content").and_then(|value| value.as_str()) == Some(expected_content)
    })
}

pub(crate) fn cleanup_child(child: &mut Child, keep: bool) {
    if keep {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}
