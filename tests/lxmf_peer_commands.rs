use lxmf::cli::app::{Cli, Command, PeerAction, PeerCommand, RuntimeContext};
use lxmf::cli::commands_peer;
use lxmf::cli::output::Output;
use lxmf::cli::profile::{init_profile, load_profile_settings, profile_paths};
use lxmf::cli::rpc_client::RpcClient;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

#[derive(Debug, Serialize)]
struct RpcResponse {
    id: u64,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: String,
    message: String,
}

#[test]
fn peer_sync_invokes_peer_sync_rpc() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("peer-test", false, None).unwrap();

    let (rpc_addr, worker) = spawn_one_rpc_server(json!({"peer": "alpha", "synced": true}));

    let settings = {
        let mut s = load_profile_settings("peer-test").unwrap();
        s.rpc = rpc_addr;
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "peer-test".into(),
            rpc: None,
            json: true,
            no_color: true,
            quiet: true,
            verbose: 0,
            command: Command::Peer(PeerCommand {
                action: PeerAction::List {
                    query: None,
                    limit: None,
                },
            }),
        },
        profile_name: "peer-test".into(),
        profile_paths: profile_paths("peer-test").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true, true),
        profile_settings: settings,
    };

    let command = PeerCommand {
        action: PeerAction::Sync {
            peer: "alpha".into(),
        },
    };

    commands_peer::run(&ctx, &command).unwrap();
    assert!(worker.join().unwrap());
    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn peer_show_supports_wrapped_list_and_name_search() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("peer-show", false, None).unwrap();

    let (rpc_addr, worker) = spawn_one_rpc_server(json!({
        "peers": [
            {"peer": "aa11", "name": "Alice Node", "last_seen": 10},
            {"peer": "bb22", "name": "Bob Node", "last_seen": 8}
        ]
    }));

    let settings = {
        let mut s = load_profile_settings("peer-show").unwrap();
        s.rpc = rpc_addr;
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "peer-show".into(),
            rpc: None,
            json: false,
            no_color: true,
            quiet: true,
            verbose: 0,
            command: Command::Peer(PeerCommand {
                action: PeerAction::Show {
                    selector: "alice".into(),
                    exact: false,
                },
            }),
        },
        profile_name: "peer-show".into(),
        profile_paths: profile_paths("peer-show").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(false, true, true),
        profile_settings: settings,
    };

    let command = PeerCommand {
        action: PeerAction::Show {
            selector: "alice".into(),
            exact: false,
        },
    };

    commands_peer::run(&ctx, &command).unwrap();
    assert!(worker.join().unwrap());
    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn peer_show_reports_ambiguous_selector() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("peer-ambiguous", false, None).unwrap();

    let (rpc_addr, worker) = spawn_one_rpc_server(json!({
        "peers": [
            {"peer": "aa11", "name": "Alice", "last_seen": 10},
            {"peer": "aa22", "name": "Alice Two", "last_seen": 8}
        ]
    }));

    let settings = {
        let mut s = load_profile_settings("peer-ambiguous").unwrap();
        s.rpc = rpc_addr;
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "peer-ambiguous".into(),
            rpc: None,
            json: false,
            no_color: true,
            quiet: true,
            verbose: 0,
            command: Command::Peer(PeerCommand {
                action: PeerAction::Show {
                    selector: "alice".into(),
                    exact: false,
                },
            }),
        },
        profile_name: "peer-ambiguous".into(),
        profile_paths: profile_paths("peer-ambiguous").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(false, true, true),
        profile_settings: settings,
    };

    let command = PeerCommand {
        action: PeerAction::Show {
            selector: "alice".into(),
            exact: false,
        },
    };

    let err = commands_peer::run(&ctx, &command).expect_err("ambiguous selector should fail");
    assert!(err.to_string().contains("ambiguous"));
    assert!(worker.join().unwrap());
    std::env::remove_var("LXMF_CONFIG_ROOT");
}

fn spawn_one_rpc_server(result: Value) -> (String, thread::JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        let saw_post_rpc = request.path == "/rpc" && request.http_method == "POST";

        let response = RpcResponse {
            id: 1,
            result: Some(result),
            error: None,
        };

        write_http_response(&mut stream, 200, &encode_frame(&response));
        saw_post_rpc
    });

    (format!("127.0.0.1:{}", addr.port()), worker)
}

struct HttpRequest {
    http_method: String,
    path: String,
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

    HttpRequest { http_method, path }
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
