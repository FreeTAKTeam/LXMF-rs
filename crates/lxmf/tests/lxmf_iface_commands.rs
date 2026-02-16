#![cfg(feature = "cli")]

use lxmf::cli::app::{Cli, Command, IfaceAction, IfaceCommand, IfaceMutationArgs, RuntimeContext};
use lxmf::cli::commands_iface;
use lxmf::cli::output::Output;
use lxmf::cli::profile::{init_profile, load_profile_settings, profile_paths};
use lxmf::cli::rpc_client::RpcClient;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

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
fn iface_apply_pushes_interfaces_to_rpc() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("iface-test", true, None).unwrap();

    let (rpc_addr, worker) = spawn_apply_rpc_server();

    let settings = {
        let mut s = load_profile_settings("iface-test").unwrap();
        s.rpc = rpc_addr;
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "iface-test".into(),
            rpc: None,
            json: true,
            quiet: true,
            command: Command::Iface(IfaceCommand { action: IfaceAction::List }),
        },
        profile_name: "iface-test".into(),
        profile_paths: profile_paths("iface-test").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true),
        profile_settings: settings,
    };

    commands_iface::run(
        &ctx,
        &IfaceCommand {
            action: IfaceAction::Add(IfaceMutationArgs {
                name: "uplink".into(),
                kind: "tcp_client".into(),
                host: Some("127.0.0.1".into()),
                port: Some(4242),
                enabled: true,
            }),
        },
    )
    .unwrap();

    commands_iface::run(&ctx, &IfaceCommand { action: IfaceAction::Apply { restart: false } })
        .unwrap();

    let paths = worker.join().unwrap();
    assert!(!paths.is_empty());
    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn iface_apply_restart_preserves_external_mode() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());
    init_profile("iface-external", false, None).unwrap();

    let settings = {
        let mut s = load_profile_settings("iface-external").unwrap();
        s.rpc = "127.0.0.1:9".into();
        s
    };

    let ctx = RuntimeContext {
        cli: Cli {
            profile: "iface-external".into(),
            rpc: None,
            json: true,
            quiet: true,
            command: Command::Iface(IfaceCommand { action: IfaceAction::Apply { restart: true } }),
        },
        profile_name: "iface-external".into(),
        profile_paths: profile_paths("iface-external").unwrap(),
        rpc: RpcClient::new(&settings.rpc),
        output: Output::new(true, true),
        profile_settings: settings,
    };

    let err =
        commands_iface::run(&ctx, &IfaceCommand { action: IfaceAction::Apply { restart: true } })
            .unwrap_err();
    assert!(err.to_string().contains("external mode"));

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

fn spawn_apply_rpc_server() -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let worker = thread::spawn(move || {
        let mut paths = Vec::new();
        let start = Instant::now();

        while start.elapsed() < Duration::from_secs(2) && paths.len() < 2 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(250)))
                        .expect("set read timeout");
                    let request = read_http_request(&mut stream);
                    assert_eq!(request.path, "/rpc");
                    assert_eq!(request.http_method, "POST");
                    paths.push(request.path.clone());

                    let result = if paths.len() == 1 {
                        json!({"updated": true})
                    } else {
                        json!({"reloaded": true})
                    };

                    let response =
                        RpcResponse { id: paths.len() as u64, result: Some(result), error: None };
                    write_http_response(&mut stream, 200, &encode_frame(&response));
                }
                Err(err) => panic!("accept failed: {err}"),
            }
        }

        paths
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
