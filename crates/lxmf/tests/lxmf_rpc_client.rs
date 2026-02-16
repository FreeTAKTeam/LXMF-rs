#![cfg(feature = "cli")]

use lxmf::cli::rpc_client::RpcClient;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

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

#[derive(Debug, Serialize)]
struct RpcEvent {
    event_type: String,
    payload: Value,
}

#[test]
fn rpc_client_roundtrip_and_event_polling() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let worker = thread::spawn(move || {
        for idx in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);

            match (request.http_method.as_str(), request.path.as_str()) {
                ("POST", "/rpc") => {
                    let response = RpcResponse {
                        id: (idx + 1) as u64,
                        result: Some(json!({
                            "ok": true,
                            "echo_method": "status",
                        })),
                        error: None,
                    };
                    write_http_response(&mut stream, 200, &encode_frame(&response));
                }
                ("GET", "/events") => {
                    let event = RpcEvent {
                        event_type: "outbound.progress".into(),
                        payload: json!({"progress": 42}),
                    };
                    write_http_response(&mut stream, 200, &encode_frame(&event));
                }
                _ => {
                    write_http_response(&mut stream, 404, b"not found");
                }
            }
        }
    });

    let client = RpcClient::new(&format!("127.0.0.1:{}", addr.port()));
    let result = client.call("status", None).unwrap();
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["echo_method"], Value::String("status".into()));

    let event = client.poll_event().unwrap().expect("event expected");
    assert_eq!(event.event_type, "outbound.progress");
    assert_eq!(event.payload["progress"], Value::from(42));

    worker.join().unwrap();
}

#[test]
fn rpc_client_formats_status_line_timeouts_cleanly() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let worker = thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(140));
    });

    let client = RpcClient::new_with_timeouts(
        &format!("127.0.0.1:{}", addr.port()),
        Duration::from_millis(40),
        Duration::from_millis(60),
        Duration::from_millis(60),
    );
    let err = client.call("status", None).unwrap_err().to_string();

    assert!(err.contains("rpc request failed"));
    assert!(!err.contains("Error encountered in the status line"));
    assert!(
        err.contains("did not return valid rpc/http response") || err.contains("network i/o error")
    );

    worker.join().unwrap();
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
        404 => "Not Found",
        _ => "Error",
    };
    let content_type = if status_code == 404 { "text/plain" } else { "application/msgpack" };

    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status_code,
        status_text,
        content_type,
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
