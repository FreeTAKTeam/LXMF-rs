#![cfg(feature = "cli")]

use lxmf::cli::app::{
    AnnounceAction, AnnounceCommand, Cli, Command, DaemonAction, DaemonCommand, IfaceAction,
    IfaceCommand, MessageAction, MessageCommand, MessageSendArgs, PeerAction, PeerCommand,
    PropagationAction, PropagationCommand, RuntimeContext, StampAction, StampCommand,
};
use lxmf::cli::commands_daemon;
use lxmf::cli::commands_iface;
use lxmf::cli::commands_message;
use lxmf::cli::commands_peer;
use lxmf::cli::commands_propagation;
use lxmf::cli::commands_stamp;
use lxmf::cli::output::Output;
use lxmf::cli::profile::{init_profile, load_profile_settings, profile_paths};
use lxmf::cli::rpc_client::RpcClient;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Serialize)]
struct RpcResponse {
    id: u64,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize)]
struct RpcError {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: u64,
    method: String,
    params: Option<Value>,
}

struct ExpectedCall {
    method: &'static str,
    required_param_keys: &'static [&'static str],
    result: Value,
    error: Option<RpcError>,
}

#[test]
fn message_and_announce_methods_contract() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("rpc-contract-message", false, None).unwrap();

    let expected = vec![
        ExpectedCall {
            method: "list_messages",
            required_param_keys: &[],
            result: json!({ "messages": [] }),
            error: None,
        },
        ExpectedCall {
            method: "clear_messages",
            required_param_keys: &[],
            result: json!({ "cleared": true }),
            error: None,
        },
        ExpectedCall {
            method: "announce_now",
            required_param_keys: &[],
            result: json!({ "announced": true }),
            error: None,
        },
        ExpectedCall {
            method: "send_message_v2",
            required_param_keys: &["id", "source", "destination", "title", "content"],
            result: json!({ "queued": true }),
            error: None,
        },
        ExpectedCall {
            method: "send_message_v2",
            required_param_keys: &["id", "source", "destination", "title", "content"],
            result: Value::Null,
            error: Some(RpcError {
                code: "method_not_supported".into(),
                message: "fallback to legacy send".into(),
            }),
        },
        ExpectedCall {
            method: "send_message",
            required_param_keys: &["id", "source", "destination", "title", "content"],
            result: json!({ "queued": true }),
            error: None,
        },
    ];

    let (rpc_addr, worker) = spawn_scripted_rpc_server(expected);
    let ctx = runtime_ctx("rpc-contract-message", &rpc_addr);

    commands_message::run(&ctx, &MessageCommand { action: MessageAction::List }).unwrap();

    commands_message::run(&ctx, &MessageCommand { action: MessageAction::Clear }).unwrap();

    commands_message::run_announce(&ctx, &AnnounceCommand { action: AnnounceAction::Now }).unwrap();

    commands_message::run(
        &ctx,
        &MessageCommand {
            action: MessageAction::Send(MessageSendArgs {
                id: Some("msg-v2".into()),
                source: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
                destination: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                title: "v2".into(),
                content: "hello".into(),
                fields_json: None,
                method: None,
                stamp_cost: None,
                include_ticket: false,
            }),
        },
    )
    .unwrap();

    commands_message::run(
        &ctx,
        &MessageCommand {
            action: MessageAction::Send(MessageSendArgs {
                id: Some("msg-v1".into()),
                source: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
                destination: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                title: "fallback".into(),
                content: "legacy".into(),
                fields_json: None,
                method: None,
                stamp_cost: None,
                include_ticket: false,
            }),
        },
    )
    .unwrap();

    let observed = worker.join().unwrap();
    assert_eq!(
        observed,
        vec![
            "list_messages",
            "clear_messages",
            "announce_now",
            "send_message_v2",
            "send_message_v2",
            "send_message",
        ]
    );

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn identity_resolution_contract_uses_status_fallback() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("rpc-contract-identity", false, None).unwrap();

    let expected = vec![
        ExpectedCall {
            method: "daemon_status_ex",
            required_param_keys: &[],
            result: Value::Null,
            error: Some(RpcError {
                code: "unavailable".into(),
                message: "daemon status unavailable".into(),
            }),
        },
        ExpectedCall {
            method: "status",
            required_param_keys: &[],
            result: json!({ "identity_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }),
            error: None,
        },
        ExpectedCall {
            method: "send_message_v2",
            required_param_keys: &["id", "source", "destination", "title", "content"],
            result: json!({ "queued": true }),
            error: None,
        },
    ];

    let (rpc_addr, worker) = spawn_scripted_rpc_server(expected);
    let ctx = runtime_ctx("rpc-contract-identity", &rpc_addr);

    commands_message::run(
        &ctx,
        &MessageCommand {
            action: MessageAction::Send(MessageSendArgs {
                id: Some("msg-no-source".into()),
                source: None,
                destination: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                title: "auto-source".into(),
                content: "hello".into(),
                fields_json: None,
                method: None,
                stamp_cost: None,
                include_ticket: false,
            }),
        },
    )
    .unwrap();

    let observed = worker.join().unwrap();
    assert_eq!(observed, vec!["daemon_status_ex", "status", "send_message_v2"]);

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn peer_iface_and_daemon_contract_methods() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("rpc-contract-peer", false, None).unwrap();

    let expected = vec![
        ExpectedCall {
            method: "list_peers",
            required_param_keys: &[],
            result: json!({ "peers": [] }),
            error: None,
        },
        ExpectedCall {
            method: "peer_sync",
            required_param_keys: &["peer"],
            result: json!({ "synced": true }),
            error: None,
        },
        ExpectedCall {
            method: "peer_unpeer",
            required_param_keys: &["peer"],
            result: json!({ "removed": true }),
            error: None,
        },
        ExpectedCall {
            method: "clear_peers",
            required_param_keys: &[],
            result: json!({ "cleared": true }),
            error: None,
        },
        ExpectedCall {
            method: "list_interfaces",
            required_param_keys: &[],
            result: json!({ "interfaces": [] }),
            error: None,
        },
        ExpectedCall {
            method: "daemon_status_ex",
            required_param_keys: &[],
            result: json!({ "running": true, "identity_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }),
            error: None,
        },
    ];

    let (rpc_addr, worker) = spawn_scripted_rpc_server(expected);
    let ctx = runtime_ctx("rpc-contract-peer", &rpc_addr);

    commands_peer::run(
        &ctx,
        &PeerCommand { action: PeerAction::List { query: None, limit: None } },
    )
    .unwrap();
    commands_peer::run(&ctx, &PeerCommand { action: PeerAction::Sync { peer: "alpha".into() } })
        .unwrap();
    commands_peer::run(&ctx, &PeerCommand { action: PeerAction::Unpeer { peer: "alpha".into() } })
        .unwrap();
    commands_peer::run(&ctx, &PeerCommand { action: PeerAction::Clear }).unwrap();

    commands_iface::run(&ctx, &IfaceCommand { action: IfaceAction::List }).unwrap();

    commands_daemon::run(&ctx, &DaemonCommand { action: DaemonAction::Status }).unwrap();

    let observed = worker.join().unwrap();
    assert_eq!(
        observed,
        vec![
            "list_peers",
            "peer_sync",
            "peer_unpeer",
            "clear_peers",
            "list_interfaces",
            "daemon_status_ex",
        ]
    );

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn propagation_contract_methods() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("rpc-contract-propagation", false, None).unwrap();

    let expected = vec![
        ExpectedCall {
            method: "propagation_status",
            required_param_keys: &[],
            result: json!({ "enabled": false }),
            error: None,
        },
        ExpectedCall {
            method: "propagation_enable",
            required_param_keys: &["enabled", "store_root", "target_cost"],
            result: json!({ "enabled": true }),
            error: None,
        },
        ExpectedCall {
            method: "propagation_ingest",
            required_param_keys: &["transient_id", "payload_hex"],
            result: json!({ "ok": true }),
            error: None,
        },
        ExpectedCall {
            method: "propagation_fetch",
            required_param_keys: &["transient_id"],
            result: json!({ "payload_hex": "00aa" }),
            error: None,
        },
        ExpectedCall {
            method: "list_peers",
            required_param_keys: &[],
            result: json!({ "peers": [{ "peer": "alpha" }] }),
            error: None,
        },
        ExpectedCall {
            method: "peer_sync",
            required_param_keys: &["peer"],
            result: json!({ "peer": "alpha", "synced": true }),
            error: None,
        },
    ];

    let (rpc_addr, worker) = spawn_scripted_rpc_server(expected);
    let ctx = runtime_ctx("rpc-contract-propagation", &rpc_addr);

    commands_propagation::run(&ctx, &PropagationCommand { action: PropagationAction::Status })
        .unwrap();
    commands_propagation::run(
        &ctx,
        &PropagationCommand {
            action: PropagationAction::Enable {
                enabled: true,
                store_root: Some("lxmf-store".into()),
                target_cost: Some(8),
            },
        },
    )
    .unwrap();
    commands_propagation::run(
        &ctx,
        &PropagationCommand {
            action: PropagationAction::Ingest {
                transient_id: Some("tx-1".into()),
                payload_hex: Some("00aa".into()),
            },
        },
    )
    .unwrap();
    commands_propagation::run(
        &ctx,
        &PropagationCommand { action: PropagationAction::Fetch { transient_id: "tx-1".into() } },
    )
    .unwrap();
    commands_propagation::run(&ctx, &PropagationCommand { action: PropagationAction::Sync })
        .unwrap();

    let observed = worker.join().unwrap();
    assert_eq!(
        observed,
        vec![
            "propagation_status",
            "propagation_enable",
            "propagation_ingest",
            "propagation_fetch",
            "list_peers",
            "peer_sync",
        ]
    );

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn stamp_contract_methods() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("rpc-contract-stamp", false, None).unwrap();

    let expected = vec![
        ExpectedCall {
            method: "stamp_policy_get",
            required_param_keys: &[],
            result: json!({ "target_cost": 8, "flexibility": 2 }),
            error: None,
        },
        ExpectedCall {
            method: "stamp_policy_get",
            required_param_keys: &[],
            result: json!({ "target_cost": 8, "flexibility": 2 }),
            error: None,
        },
        ExpectedCall {
            method: "stamp_policy_set",
            required_param_keys: &["target_cost", "flexibility"],
            result: json!({ "target_cost": 10, "flexibility": 4 }),
            error: None,
        },
        ExpectedCall {
            method: "ticket_generate",
            required_param_keys: &["destination", "ttl_secs"],
            result: json!({ "ticket": "abcd" }),
            error: None,
        },
        ExpectedCall {
            method: "stamp_policy_get",
            required_param_keys: &[],
            result: json!({ "target_cost": 10, "flexibility": 4 }),
            error: None,
        },
    ];

    let (rpc_addr, worker) = spawn_scripted_rpc_server(expected);
    let ctx = runtime_ctx("rpc-contract-stamp", &rpc_addr);

    commands_stamp::run(&ctx, &StampCommand { action: StampAction::Target }).unwrap();
    commands_stamp::run(&ctx, &StampCommand { action: StampAction::Get }).unwrap();
    commands_stamp::run(
        &ctx,
        &StampCommand { action: StampAction::Set { target_cost: Some(10), flexibility: Some(4) } },
    )
    .unwrap();
    commands_stamp::run(
        &ctx,
        &StampCommand {
            action: StampAction::GenerateTicket {
                destination: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                ttl_secs: Some(3600),
            },
        },
    )
    .unwrap();
    commands_stamp::run(&ctx, &StampCommand { action: StampAction::Cache }).unwrap();

    let observed = worker.join().unwrap();
    assert_eq!(
        observed,
        vec![
            "stamp_policy_get",
            "stamp_policy_get",
            "stamp_policy_set",
            "ticket_generate",
            "stamp_policy_get",
        ]
    );

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.get_or_init(|| Mutex::new(())).lock().expect("test mutex lock")
}

fn runtime_ctx(profile_name: &str, rpc_addr: &str) -> RuntimeContext {
    let mut settings = load_profile_settings(profile_name).expect("load profile settings");
    settings.rpc = rpc_addr.to_string();

    RuntimeContext {
        cli: Cli {
            profile: profile_name.to_string(),
            rpc: None,
            json: true,
            quiet: true,
            command: Command::Message(MessageCommand { action: MessageAction::List }),
        },
        profile_name: profile_name.to_string(),
        profile_paths: profile_paths(profile_name).expect("profile paths"),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true),
        profile_settings: settings,
    }
}

fn spawn_scripted_rpc_server(
    expected: Vec<ExpectedCall>,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();

    let worker = thread::spawn(move || {
        let mut observed = Vec::new();
        let start = Instant::now();
        let mut idx = 0usize;

        while idx < expected.len() && start.elapsed() < Duration::from_secs(10) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_http_request(&mut stream);
                    assert_eq!(request.path, "/rpc");
                    assert_eq!(request.http_method, "POST");

                    let rpc: RpcRequest = decode_frame(&request.body);
                    let expected_call = &expected[idx];
                    assert_eq!(
                        rpc.method, expected_call.method,
                        "rpc method mismatch at call {idx}"
                    );
                    assert_params(
                        &rpc.params,
                        expected_call.required_param_keys,
                        expected_call.method,
                    );

                    observed.push(rpc.method.clone());

                    let response = RpcResponse {
                        id: rpc.id,
                        result: if expected_call.error.is_none() {
                            Some(expected_call.result.clone())
                        } else {
                            None
                        },
                        error: expected_call.error.clone(),
                    };
                    write_http_response(&mut stream, 200, &encode_frame(&response));
                    idx += 1;
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("accept failed: {err}"),
            }
        }

        assert_eq!(idx, expected.len(), "did not observe all expected rpc calls");
        observed
    });

    (format!("127.0.0.1:{}", addr.port()), worker)
}

fn assert_params(params: &Option<Value>, required_keys: &[&str], method: &str) {
    if required_keys.is_empty() {
        assert!(params.is_none(), "method '{}' expected no params but got {:?}", method, params);
        return;
    }

    let object = params
        .as_ref()
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("method '{}' expected object params", method));
    for key in required_keys {
        assert!(object.contains_key(*key), "method '{}' missing param key '{}'", method, key);
    }
}

struct HttpRequest {
    http_method: String,
    path: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> HttpRequest {
    let mut bytes = Vec::new();
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let mut buf = [0u8; 1024];
        let read = stream.read(&mut buf).unwrap();
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..read]);

        if header_end.is_none() {
            if let Some(pos) = find_header_end(&bytes) {
                header_end = Some(pos);
                let headers = String::from_utf8_lossy(&bytes[..pos]);
                content_length = parse_content_length(&headers);
            }
        }

        if let Some(pos) = header_end {
            let body_start = pos + 4;
            if bytes.len() >= body_start + content_length {
                break;
            }
        }
    }

    let header_end = header_end.expect("valid http request headers");
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let http_method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let body_start = header_end + 4;
    let body = if body_start <= bytes.len() { bytes[body_start..].to_vec() } else { Vec::new() };

    HttpRequest { http_method, path, body }
}

fn write_http_response(stream: &mut TcpStream, status_code: u16, body: &[u8]) {
    let status_text = match status_code {
        200 => "OK",
        204 => "No Content",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status_code,
        status_text,
        body.len()
    );
    stream.write_all(header.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0)
}

fn encode_frame<T: Serialize>(value: &T) -> Vec<u8> {
    let payload = rmp_serde::to_vec(value).unwrap();
    let mut framed = Vec::with_capacity(payload.len() + 4);
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend_from_slice(&payload);
    framed
}

fn decode_frame<T: for<'de> Deserialize<'de>>(framed: &[u8]) -> T {
    assert!(framed.len() >= 4, "missing frame length");
    let mut len_buf = [0u8; 4];
    len_buf.copy_from_slice(&framed[..4]);
    let len = u32::from_be_bytes(len_buf) as usize;
    assert!(framed.len() >= 4 + len, "incomplete frame");
    rmp_serde::from_slice(&framed[4..4 + len]).unwrap()
}
