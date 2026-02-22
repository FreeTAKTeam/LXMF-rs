use clap::{Parser, ValueEnum};
use rns_rpc::e2e_harness::{
    build_daemon_args, build_http_post, build_rpc_frame, build_send_params,
    build_tcp_client_config, is_ready_line, parse_http_response_body, parse_rpc_frame,
    timestamp_millis,
};
use rns_rpc::rpc::replay::{execute_trace, load_trace_file, save_capture_file};
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
        #[arg(long = "mode", value_enum)]
        modes: Vec<DeliveryMode>,
    },
    MeshSim {
        #[arg(long, default_value_t = 5)]
        nodes: usize,
        #[arg(long, default_value_t = 4340)]
        base_rpc_port: u16,
        #[arg(long, default_value_t = 90)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        keep: bool,
        #[arg(long = "mode", value_enum)]
        modes: Vec<DeliveryMode>,
    },
    Replay {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        capture_out: Option<PathBuf>,
        #[arg(long, default_value = "replay-identity")]
        identity_hash: String,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Hash)]
enum DeliveryMode {
    Direct,
    Opportunistic,
    Propagated,
    Paper,
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
        Command::E2e { a_port, b_port, timeout_secs, keep, modes } => {
            run_e2e(a_port, b_port, timeout_secs, keep, modes)
        }
        Command::MeshSim { nodes, base_rpc_port, timeout_secs, keep, modes } => {
            run_mesh_sim(nodes, base_rpc_port, timeout_secs, keep, modes)
        }
        Command::Replay { trace, capture_out, identity_hash } => {
            run_replay(trace, capture_out, identity_hash)
        }
    }
}

fn run_replay(
    trace: PathBuf,
    capture_out: Option<PathBuf>,
    identity_hash: String,
) -> io::Result<()> {
    let trace_data = load_trace_file(&trace).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to load replay trace '{}': {error}", trace.display()),
        )
    })?;
    let daemon = rns_rpc::RpcDaemon::test_instance_with_identity(identity_hash.as_str());
    let capture = execute_trace(&daemon, &trace_data).map_err(io::Error::other)?;
    if let Some(path) = capture_out {
        save_capture_file(&path, &capture).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to write replay capture '{}': {error}", path.display()),
            )
        })?;
    }
    println!(
        "REPLAY ok: trace='{}' steps={} digest={}",
        capture.trace_name, capture.steps_executed, capture.response_digest_sha256
    );
    Ok(())
}

fn run_e2e(
    a_port: u16,
    b_port: u16,
    timeout_secs: u64,
    keep: bool,
    modes: Vec<DeliveryMode>,
) -> io::Result<()> {
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

    let selected_modes = selected_delivery_modes(&modes);
    for mode in selected_modes {
        match mode {
            DeliveryMode::Direct | DeliveryMode::Opportunistic | DeliveryMode::Propagated => {
                run_delivery_mode(
                    mode,
                    &a_rpc,
                    &b_rpc,
                    &a_destination_for_b,
                    &b_destination_for_a,
                    timeout,
                    &mut req_id,
                )?;
                run_delivery_mode(
                    mode,
                    &b_rpc,
                    &a_rpc,
                    &b_destination_for_a,
                    &a_destination_for_b,
                    timeout,
                    &mut req_id,
                )?;
            }
            DeliveryMode::Paper => {
                run_paper_workflow(
                    &a_rpc,
                    &b_rpc,
                    &a_destination_for_b,
                    &b_destination_for_a,
                    timeout,
                    &mut req_id,
                )?;
            }
        }
    }

    cleanup_child(&mut a_child, keep);
    cleanup_child(&mut b_child, keep);
    println!("E2E ok: peer discovery A<->B succeeded");
    println!("E2E ok: compatibility delivery modes completed");
    Ok(())
}

struct MeshNodeProcess {
    rpc: String,
    destination_hash: String,
    child: Child,
}

fn run_mesh_sim(
    nodes: usize,
    base_rpc_port: u16,
    timeout_secs: u64,
    keep: bool,
    modes: Vec<DeliveryMode>,
) -> io::Result<()> {
    if !(3..=10).contains(&nodes) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "nodes must be in range 3..=10"));
    }

    let timeout = Duration::from_secs(timeout_secs);
    let mut reserved_ports = HashSet::new();
    let mut rpc_listeners = Vec::with_capacity(nodes);
    let mut rpc_ports = Vec::with_capacity(nodes);
    let mut transport_listeners = Vec::with_capacity(nodes);
    let mut transport_ports = Vec::with_capacity(nodes);

    for idx in 0..nodes {
        let offset = u16::try_from(idx).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "nodes index exceeds u16 range")
        })?;
        let preferred_rpc = base_rpc_port
            .checked_add(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rpc port overflow"))?;
        let rpc_listener = reserve_port(preferred_rpc, &reserved_ports)?;
        let rpc_port = rpc_listener.local_addr()?.port();
        reserved_ports.insert(rpc_port);
        rpc_ports.push(rpc_port);
        rpc_listeners.push(rpc_listener);
    }

    for rpc_port in &rpc_ports {
        let preferred_transport = derive_preferred_transport_port(*rpc_port, 100)?;
        let transport_listener = reserve_port(preferred_transport, &reserved_ports)?;
        let transport_port = transport_listener.local_addr()?.port();
        reserved_ports.insert(transport_port);
        transport_ports.push(transport_port);
        transport_listeners.push(transport_listener);
    }

    let mut temp_dirs = Vec::with_capacity(nodes);
    let mut db_paths = Vec::with_capacity(nodes);
    let mut config_paths = Vec::with_capacity(nodes);
    for idx in 0..nodes {
        let dir = tempfile::TempDir::new()?;
        let db_path = dir.path().join(format!("reticulum-{idx}.db"));
        let config_path = dir.path().join(format!("reticulum-{idx}.toml"));
        fs::write(&config_path, build_mesh_client_config(idx, &transport_ports))?;
        db_paths.push(db_path);
        config_paths.push(config_path);
        temp_dirs.push(dir);
    }

    drop(rpc_listeners);
    drop(transport_listeners);

    let mut node_processes = Vec::with_capacity(nodes);
    for idx in 0..nodes {
        let rpc = format!("127.0.0.1:{}", rpc_ports[idx]);
        let transport = format!("127.0.0.1:{}", transport_ports[idx]);
        let mut child = match spawn_daemon(&rpc, &db_paths[idx], &transport, &config_paths[idx]) {
            Ok(child) => child,
            Err(err) => {
                cleanup_mesh_children(&mut node_processes, keep);
                return Err(err);
            }
        };
        let destination_hash = match wait_for_ready(
            child.stdout.take().ok_or_else(|| io::Error::other("missing daemon stdout"))?,
            timeout,
        ) {
            Ok(Some(hash)) => hash,
            Ok(None) => {
                cleanup_mesh_children(&mut node_processes, keep);
                cleanup_child(&mut child, keep);
                return Err(io::Error::other("daemon did not report destination hash"));
            }
            Err(err) => {
                cleanup_mesh_children(&mut node_processes, keep);
                cleanup_child(&mut child, keep);
                return Err(err);
            }
        };

        node_processes.push(MeshNodeProcess { rpc, destination_hash, child });
    }

    let mut request_id = 10_u64;
    let selected_modes = selected_mesh_delivery_modes(&modes);
    let first = 0_usize;
    let last = nodes - 1;

    let result = (|| -> io::Result<()> {
        for node in &node_processes {
            rpc_call(&node.rpc, request_id, "announce_now", None)?;
            request_id = request_id.wrapping_add(1);
        }

        for node in &node_processes {
            let discovered =
                poll_for_any_peer(&node.rpc, timeout, request_id, Some(&node.destination_hash))?;
            if discovered.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mesh propagation failed: a node did not discover any peer",
                ));
            }
            request_id = request_id.wrapping_add(1);
        }

        for mode in selected_modes {
            match mode {
                DeliveryMode::Direct | DeliveryMode::Opportunistic | DeliveryMode::Propagated => {
                    run_delivery_mode(
                        mode,
                        &node_processes[first].rpc,
                        &node_processes[last].rpc,
                        &node_processes[first].destination_hash,
                        &node_processes[last].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                    run_delivery_mode(
                        mode,
                        &node_processes[last].rpc,
                        &node_processes[first].rpc,
                        &node_processes[last].destination_hash,
                        &node_processes[first].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                }
                DeliveryMode::Paper => {
                    run_paper_workflow(
                        &node_processes[first].rpc,
                        &node_processes[last].rpc,
                        &node_processes[first].destination_hash,
                        &node_processes[last].destination_hash,
                        timeout,
                        &mut request_id,
                    )?;
                }
            }
        }

        println!("MESH ok: nodes={} announce propagation established across mesh", nodes);
        println!("MESH ok: multi-hop delivery workflows completed");
        Ok(())
    })();

    cleanup_mesh_children(&mut node_processes, keep);
    drop(temp_dirs);
    result
}

fn cleanup_mesh_children(node_processes: &mut [MeshNodeProcess], keep: bool) {
    for node in node_processes {
        cleanup_child(&mut node.child, keep);
    }
}

fn build_mesh_client_config(node_index: usize, transport_ports: &[u16]) -> String {
    let node_count = transport_ports.len();
    let next = (node_index + 1) % node_count;
    let previous = (node_index + node_count - 1) % node_count;
    let mut neighbors = vec![next];
    if previous != next {
        neighbors.push(previous);
    }

    let mut config = String::new();
    for neighbor in neighbors {
        config.push_str(&format!(
            "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nhost = \"127.0.0.1\"\nport = {}\n\n",
            transport_ports[neighbor]
        ));
    }
    config
}

fn selected_mesh_delivery_modes(modes: &[DeliveryMode]) -> Vec<DeliveryMode> {
    if modes.is_empty() {
        return vec![DeliveryMode::Direct];
    }
    selected_delivery_modes(modes)
}

fn selected_delivery_modes(modes: &[DeliveryMode]) -> Vec<DeliveryMode> {
    if modes.is_empty() {
        return vec![
            DeliveryMode::Direct,
            DeliveryMode::Opportunistic,
            DeliveryMode::Propagated,
            DeliveryMode::Paper,
        ];
    }
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for mode in modes {
        if seen.insert(*mode) {
            selected.push(*mode);
        }
    }
    selected
}

fn mode_label(mode: DeliveryMode) -> &'static str {
    match mode {
        DeliveryMode::Direct => "direct",
        DeliveryMode::Opportunistic => "opportunistic",
        DeliveryMode::Propagated => "propagated",
        DeliveryMode::Paper => "paper",
    }
}

fn build_mode_send_params(
    message_id: &str,
    source: &str,
    destination: &str,
    content: &str,
    mode: DeliveryMode,
) -> serde_json::Value {
    let mut params = build_send_params(message_id, source, destination, content);
    if let Some(object) = params.as_object_mut() {
        object.insert("method".to_string(), serde_json::json!(mode_label(mode)));
        if matches!(mode, DeliveryMode::Propagated) {
            object.insert("include_ticket".to_string(), serde_json::json!(true));
            object.insert("try_propagation_on_fail".to_string(), serde_json::json!(true));
            object.insert("stamp_cost".to_string(), serde_json::json!(8));
        }
    }
    params
}

fn run_delivery_mode(
    mode: DeliveryMode,
    sender_rpc: &str,
    receiver_rpc: &str,
    sender_destination: &str,
    receiver_destination: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let label = mode_label(mode);
    let message_id = format!("e2e-{}-{}", label, timestamp_millis());
    let content = format!("hello from rnx e2e ({label})");
    let params = build_mode_send_params(
        &message_id,
        sender_destination,
        receiver_destination,
        &content,
        mode,
    );
    let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(params))?;
    ensure_rpc_ok(response, format!("send_message_v2 ({label})").as_str())?;
    *request_id = (*request_id).wrapping_add(1);

    let delivered = poll_for_inbound_content(receiver_rpc, &content, timeout, *request_id)?;
    if !delivered {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("delivery mode '{label}' did not deliver message '{message_id}'"),
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    let trace_contains_status =
        poll_for_delivery_trace_status(sender_rpc, &message_id, label, timeout, *request_id)?;
    if !trace_contains_status {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("delivery trace for '{message_id}' did not contain mode '{label}'"),
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    println!("E2E ok: mode={} message {} delivered", label, message_id);
    Ok(())
}

fn run_paper_workflow(
    sender_rpc: &str,
    receiver_rpc: &str,
    sender_destination: &str,
    receiver_destination: &str,
    timeout: Duration,
    request_id: &mut u64,
) -> io::Result<()> {
    let message_id = format!("e2e-paper-{}", timestamp_millis());
    let content = "hello from rnx e2e (paper)";
    let send_params = build_mode_send_params(
        &message_id,
        sender_destination,
        receiver_destination,
        content,
        DeliveryMode::Paper,
    );
    let response = rpc_call(sender_rpc, *request_id, "send_message_v2", Some(send_params))?;
    ensure_rpc_ok(response, "send_message_v2 (paper)")?;
    *request_id = (*request_id).wrapping_add(1);

    let delivered = poll_for_inbound_content(receiver_rpc, content, timeout, *request_id)?;
    if !delivered {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "paper workflow did not deliver baseline message",
        ));
    }
    *request_id = (*request_id).wrapping_add(1);

    let paper_encode_response = rpc_call(
        sender_rpc,
        *request_id,
        "sdk_paper_encode_v2",
        Some(serde_json::json!({ "message_id": message_id })),
    )?;
    let paper_encode_result = ensure_rpc_ok(paper_encode_response, "sdk_paper_encode_v2")?
        .ok_or_else(|| io::Error::other("sdk_paper_encode_v2 missing result body"))?;
    let uri = paper_encode_result
        .get("envelope")
        .and_then(|value| value.get("uri"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| io::Error::other("sdk_paper_encode_v2 missing envelope uri"))?
        .to_string();
    *request_id = (*request_id).wrapping_add(1);

    let paper_decode_response = rpc_call(
        receiver_rpc,
        *request_id,
        "sdk_paper_decode_v2",
        Some(serde_json::json!({ "uri": uri })),
    )?;
    let paper_decode_result = ensure_rpc_ok(paper_decode_response, "sdk_paper_decode_v2")?
        .ok_or_else(|| io::Error::other("sdk_paper_decode_v2 missing result body"))?;
    let accepted =
        paper_decode_result.get("accepted").and_then(|value| value.as_bool()).unwrap_or(false);
    if !accepted {
        return Err(io::Error::other("sdk_paper_decode_v2 returned accepted=false"));
    }
    *request_id = (*request_id).wrapping_add(1);

    println!("E2E ok: mode=paper message {} encoded/decoded", message_id);
    Ok(())
}

fn poll_for_delivery_trace_status(
    rpc: &str,
    message_id: &str,
    expected_mode: &str,
    timeout: Duration,
    mut request_id: u64,
) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    let expected_status = format!("sent: {expected_mode}");
    loop {
        let response = rpc_call(
            rpc,
            request_id,
            "message_delivery_trace",
            Some(serde_json::json!({ "message_id": message_id })),
        )?;
        request_id = request_id.wrapping_add(1);
        let result = ensure_rpc_ok(response, "message_delivery_trace")?;
        let has_expected_status = result
            .and_then(|value| value.get("transitions").cloned())
            .and_then(|value| value.as_array().cloned())
            .map(|transitions| {
                transitions.iter().any(|transition| {
                    transition
                        .get("status")
                        .and_then(|value| value.as_str())
                        .is_some_and(|status| status.contains(&expected_status))
                })
            })
            .unwrap_or(false);
        if has_expected_status {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn ensure_rpc_ok(
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
) -> io::Result<rns_rpc::RpcResponse> {
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
fn cleanup_child(child: &mut Child, keep: bool) {
    if keep {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}
