use clap::Parser;
use reticulum::e2e_harness::{
    build_daemon_args, build_http_post, build_rpc_frame, build_send_params,
    build_tcp_client_config, is_ready_line, parse_http_response_body, parse_rpc_frame,
    timestamp_millis,
};
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(name = "rnx")]
struct Cli {
    #[arg(long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    E2e {
        #[arg(long, default_value_t = 4243)]
        a_port: u16,
        #[arg(long, default_value_t = 4244)]
        b_port: u16,
        #[arg(long, default_value_t = 60)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        keep: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("rnx error: {}", err);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> io::Result<()> {
    match cli.command {
        Command::E2e { a_port, b_port, timeout_secs, keep } => {
            run_e2e(a_port, b_port, timeout_secs, keep)
        }
    }
}

fn run_e2e(a_port: u16, b_port: u16, timeout_secs: u64, keep: bool) -> io::Result<()> {
    let timeout = Duration::from_secs(timeout_secs);
    let mut reserved_ports = HashSet::new();
    let a_rpc_listener = reserve_port(a_port, &reserved_ports)?;
    let a_rpc_port = a_rpc_listener.local_addr()?.port();
    reserved_ports.insert(a_rpc_port);
    let b_rpc_listener = reserve_port(b_port, &reserved_ports)?;
    let b_rpc_port = b_rpc_listener.local_addr()?.port();
    reserved_ports.insert(b_rpc_port);

    let a_transport_listener =
        reserve_port(derive_preferred_transport_port(a_rpc_port, 100)?, &reserved_ports)?;
    let a_transport_port = a_transport_listener.local_addr()?.port();
    reserved_ports.insert(a_transport_port);
    let b_transport_listener =
        reserve_port(derive_preferred_transport_port(b_rpc_port, 100)?, &reserved_ports)?;
    let b_transport_port = b_transport_listener.local_addr()?.port();

    let a_rpc = format!("127.0.0.1:{}", a_rpc_port);
    let b_rpc = format!("127.0.0.1:{}", b_rpc_port);
    let a_transport = format!("127.0.0.1:{}", a_transport_port);
    let b_transport = format!("127.0.0.1:{}", b_transport_port);

    let a_dir = tempfile::TempDir::new()?;
    let b_dir = tempfile::TempDir::new()?;
    let a_db = a_dir.path().join("reticulum.db");
    let b_db = b_dir.path().join("reticulum.db");
    let a_config = a_dir.path().join("reticulum.toml");
    let b_config = b_dir.path().join("reticulum.toml");

    fs::write(&a_config, build_tcp_client_config("127.0.0.1", b_transport_port))?;
    fs::write(&b_config, build_tcp_client_config("127.0.0.1", a_transport_port))?;

    drop(a_rpc_listener);
    drop(a_transport_listener);
    let mut a_child = spawn_daemon(&a_rpc, &a_db, &a_transport, &a_config)?;
    let a_destination_hash = wait_for_ready(
        a_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
        timeout,
    );
    let a_destination_hash = match a_destination_hash {
        Ok(hash) => hash,
        Err(err) => {
            cleanup_child(&mut a_child, keep);
            return Err(err);
        }
    };

    drop(b_rpc_listener);
    drop(b_transport_listener);
    let mut b_child = spawn_daemon(&b_rpc, &b_db, &b_transport, &b_config)?;
    let b_destination_hash = wait_for_ready(
        b_child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
        timeout,
    );
    let b_destination_hash = match b_destination_hash {
        Ok(hash) => hash,
        Err(err) => {
            cleanup_child(&mut a_child, keep);
            cleanup_child(&mut b_child, keep);
            return Err(err);
        }
    };

    let mut req_id = 1u64;
    rpc_call(&b_rpc, req_id, "announce_now", None)?;
    req_id = req_id.wrapping_add(1);
    let b_destination_for_a =
        poll_for_any_peer(&a_rpc, timeout, req_id, a_destination_hash.as_deref())?;
    let Some(b_destination_for_a) = b_destination_for_a else {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon A did not discover daemon B"));
    };
    req_id = req_id.wrapping_add(1);

    rpc_call(&a_rpc, req_id, "announce_now", None)?;
    req_id = req_id.wrapping_add(1);
    let a_destination_for_b =
        poll_for_any_peer(&b_rpc, timeout, req_id, b_destination_hash.as_deref())?;
    let Some(a_destination_for_b) = a_destination_for_b else {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon B did not discover daemon A"));
    };
    req_id = req_id.wrapping_add(1);

    let outbound_id = format!("e2e-outbound-{}", timestamp_millis());
    let reply_id = format!("e2e-reply-{}", timestamp_millis());
    let content = "hello from rnx e2e";
    let reply_content = "reply from rnx e2e";

    let send_params =
        build_send_params(&outbound_id, &a_destination_for_b, &b_destination_for_a, content);
    rpc_call(&a_rpc, req_id, "send_message_v2", Some(send_params))?;
    req_id = req_id.wrapping_add(1);
    let found = poll_for_inbound_content(&b_rpc, content, timeout, req_id)?;
    if !found {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "message from daemon A to daemon B not delivered",
        ));
    }
    req_id = req_id.wrapping_add(1);

    let reply_params =
        build_send_params(&reply_id, &b_destination_for_a, &a_destination_for_b, reply_content);
    rpc_call(&b_rpc, req_id, "send_message_v2", Some(reply_params))?;
    req_id = req_id.wrapping_add(1);
    let reply_found = poll_for_inbound_content(&a_rpc, reply_content, timeout, req_id)?;
    if !reply_found {
        cleanup_child(&mut a_child, keep);
        cleanup_child(&mut b_child, keep);
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "message from daemon B to daemon A not delivered",
        ));
    }

    cleanup_child(&mut a_child, keep);
    cleanup_child(&mut b_child, keep);
    println!("E2E ok: peer discovery A<->B succeeded");
    println!("E2E ok: message {} delivered A->B", outbound_id);
    println!("E2E ok: message {} delivered B->A", reply_id);
    Ok(())
}

fn spawn_daemon(rpc: &str, db_path: &Path, transport: &str, config: &Path) -> io::Result<Child> {
    let mut cmd = ProcessCommand::new(reticulumd_path()?);
    cmd.args(build_daemon_args(
        rpc,
        &db_path.to_string_lossy(),
        0,
        Some(transport),
        Some(&config.to_string_lossy()),
    ));
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.spawn()
}

fn derive_preferred_transport_port(rpc_port: u16, offset: u16) -> io::Result<u16> {
    rpc_port.checked_add(offset).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "transport port overflow derived from rpc port")
    })
}

fn reserve_port(preferred: u16, reserved: &HashSet<u16>) -> io::Result<TcpListener> {
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

fn wait_for_ready<R: Read + Send + 'static>(
    reader: R,
    timeout: Duration,
) -> io::Result<Option<String>> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(reader).lines();
        while let Some(Ok(line)) = lines.next() {
            let _ = tx.send(line);
        }
    });

    let deadline = Instant::now() + timeout;
    let mut local_destination_hash = None;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon did not become ready"));
        }
        let remaining = deadline.saturating_duration_since(now);
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                if local_destination_hash.is_none() {
                    local_destination_hash = parse_delivery_destination_hash(&line);
                }
                if is_ready_line(&line) {
                    return Ok(local_destination_hash);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "daemon stdout closed"));
            }
        }
    }
}

fn rpc_call(
    rpc: &str,
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<reticulum::rpc::RpcResponse> {
    let frame = build_rpc_frame(id, method, params)?;
    let request = build_http_post("/rpc", rpc, &frame);
    let mut stream = TcpStream::connect(rpc)?;
    stream.write_all(&request)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let body = parse_http_response_body(&response)?;
    parse_rpc_frame(&body)
}

fn poll_for_inbound_content(
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

fn poll_for_any_peer(
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

fn first_peer(
    response: &reticulum::rpc::RpcResponse,
    exclude_peer: Option<&str>,
) -> Option<String> {
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

fn inbound_content_present(response: &reticulum::rpc::RpcResponse, expected_content: &str) -> bool {
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
fn cleanup_child(child: &mut Child, keep: bool) {
    if keep {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}
