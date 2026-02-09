use lxmf::cli::app::RuntimeContext;
use lxmf::cli::app::{Cli, Command, MessageAction, MessageCommand, MessageSendArgs};
use lxmf::cli::commands_message;
use lxmf::cli::contacts::{save_contacts, ContactEntry};
use lxmf::cli::output::Output;
use lxmf::cli::profile::{init_profile, load_profile_settings, profile_paths};
use lxmf::cli::rpc_client::RpcClient;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Deserialize)]
struct RpcRequest {
    #[allow(dead_code)]
    id: u64,
    #[allow(dead_code)]
    method: String,
    params: Option<Value>,
}

#[test]
fn message_send_uses_v2_when_available() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("msg-test", false, None).unwrap();

    let (rpc_addr, worker) = spawn_one_rpc_server(json!({"id": "m-1", "queued": true}));

    let settings = {
        let mut s = load_profile_settings("msg-test").unwrap();
        s.rpc = rpc_addr;
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "msg-test".into(),
            rpc: None,
            json: true,
            no_color: true,
            quiet: true,
            verbose: 0,
            command: Command::Message(MessageCommand {
                action: MessageAction::List,
            }),
        },
        profile_name: "msg-test".into(),
        profile_paths: profile_paths("msg-test").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true, true),
        profile_settings: settings,
    };

    let command = MessageCommand {
        action: MessageAction::Send(MessageSendArgs {
            id: Some("m-1".into()),
            source: Some("aa".into()),
            destination: "bb".into(),
            title: "hello".into(),
            content: "world".into(),
            fields_json: None,
            method: None,
            stamp_cost: None,
            include_ticket: false,
        }),
    };

    commands_message::run(&ctx, &command).unwrap();
    let saw_post_rpc = worker.join().unwrap();
    assert!(saw_post_rpc);
    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn message_send_resolves_contact_alias_destination() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("msg-contact", false, None).unwrap();
    save_contacts(
        "msg-contact",
        &[ContactEntry {
            alias: "alice".into(),
            hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            notes: None,
        }],
    )
    .unwrap();

    let (rpc_addr, worker) = spawn_one_rpc_server_with_params(json!({"ok": true}));
    let settings = {
        let mut s = load_profile_settings("msg-contact").unwrap();
        s.rpc = rpc_addr;
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "msg-contact".into(),
            rpc: None,
            json: true,
            no_color: true,
            quiet: true,
            verbose: 0,
            command: Command::Message(MessageCommand {
                action: MessageAction::List,
            }),
        },
        profile_name: "msg-contact".into(),
        profile_paths: profile_paths("msg-contact").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true, true),
        profile_settings: settings,
    };

    let command = MessageCommand {
        action: MessageAction::Send(MessageSendArgs {
            id: Some("m-contact".into()),
            source: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()),
            destination: "@alice".into(),
            title: String::new(),
            content: "hello".into(),
            fields_json: None,
            method: None,
            stamp_cost: None,
            include_ticket: false,
        }),
    };

    commands_message::run(&ctx, &command).unwrap();
    let (saw_post_rpc, params) = worker.join().unwrap();
    assert!(saw_post_rpc);
    assert_eq!(
        params
            .get("destination")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
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

fn spawn_one_rpc_server_with_params(result: Value) -> (String, thread::JoinHandle<(bool, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        let saw_post_rpc = request.path == "/rpc" && request.http_method == "POST";

        let parsed_request: RpcRequest = decode_frame(&request.body);
        let params = parsed_request.params.unwrap_or_else(|| json!({}));

        let response = RpcResponse {
            id: 1,
            result: Some(result),
            error: None,
        };

        write_http_response(&mut stream, 200, &encode_frame(&response));
        (saw_post_rpc, params)
    });

    (format!("127.0.0.1:{}", addr.port()), worker)
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
    let body = if body_start <= bytes.len() {
        bytes[body_start..].to_vec()
    } else {
        Vec::new()
    };

    HttpRequest {
        http_method,
        path,
        body,
    }
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

fn decode_frame<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> T {
    let payload_len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
    let payload = &bytes[4..4 + payload_len];
    rmp_serde::from_slice(payload).unwrap()
}
