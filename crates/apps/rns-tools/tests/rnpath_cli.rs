use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use rns_rpc::rpc::codec;
use rns_rpc::{RpcError, RpcResponse};
use serde_json::json;

#[test]
fn rnpath_help_exposes_path_discovery_rpc_options() {
    let output = Command::new(rnpath_bin()).arg("--help").output().expect("run rnpath-rs help");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Request Reticulum path discovery through daemon RPC"));
    assert!(stdout.contains("DESTINATION_HASH"));
    assert!(stdout.contains("--rpc <ADDR>"));
    assert!(stdout.contains("127.0.0.1:4243"));
    #[cfg(unix)]
    assert!(stdout.contains("--rpc-unix <PATH>"));
    assert!(stdout.contains("--timeout"));
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("--on-iface"));
    assert!(stdout.contains("--tag-hex"));
}

#[test]
fn rnpath_rejects_malformed_destination_hash_before_backend_work() {
    let output =
        Command::new(rnpath_bin()).arg("not-a-destination").output().expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("destination hash must be 32 hexadecimal characters"));
}

#[test]
fn rnpath_sends_request_path_rpc_and_renders_human_summary() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "request_path");
        let params = request.params.expect("params");
        assert_eq!(params["destination_hash"].as_str(), Some("00112233445566778899aabbccddeeff"));
        assert_eq!(params["timeout_secs"].as_u64(), Some(5));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "status": "found",
                "requested": true,
                "path_found": true,
            })),
            error: None,
        }
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("--timeout")
        .arg("5")
        .output()
        .expect("run rnpath-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Path request: 00112233445566778899aabbccddeeff"));
    assert!(stdout.contains("status=found"));
    assert!(stdout.contains("requested=true"));
    assert!(stdout.contains("path_found=true"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnpath_sends_scoped_request_options() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "request_path");
        let params = request.params.expect("params");
        assert_eq!(params["destination_hash"].as_str(), Some("00112233445566778899aabbccddeeff"));
        assert_eq!(params["on_iface"].as_str(), Some("aabbccddeeff00112233445566778899"));
        assert_eq!(params["tag_hex"].as_str(), Some("01020304"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "status": "found",
                "requested": true,
                "path_found": true,
                "on_iface": "aabbccddeeff00112233445566778899",
                "tag_hex": "01020304",
            })),
            error: None,
        }
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("--on-iface")
        .arg("AABBCCDDEEFF00112233445566778899")
        .arg("--tag-hex")
        .arg("01020304")
        .output()
        .expect("run rnpath-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("on_iface=aabbccddeeff00112233445566778899"));
    assert!(stdout.contains("tag_hex=01020304"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnpath_json_prints_daemon_result_verbatim() {
    let rpc = spawn_mock_rpc(|request| RpcResponse {
        id: request.id,
        result: Some(json!({
            "destination_hash": "00112233445566778899aabbccddeeff",
            "status": "found",
            "path_found": true,
            "next_hop": "8899aabbccddeeff0011223344556677",
            "hops": 2,
        })),
        error: None,
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899AABBCCDDEEFF")
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("--json")
        .output()
        .expect("run rnpath-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["status"].as_str(), Some("found"));
    assert_eq!(value["hops"].as_u64(), Some(2));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnpath_uses_default_tcp_rpc_when_no_transport_flag_is_supplied() {
    let Ok(listener) = TcpListener::bind("127.0.0.1:4243") else {
        eprintln!("skipping default TCP RPC test because 127.0.0.1:4243 is already in use");
        return;
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.contains("\r\nHost: 127.0.0.1:4243\r\n"), "request: {request_text}");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.method, "request_path");
        let params = rpc_request.params.expect("params");
        assert_eq!(params["destination_hash"].as_str(), Some("00112233445566778899aabbccddeeff"));

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "status": "found",
                "path_found": true,
                "next_hop": "8899aabbccddeeff0011223344556677",
            })),
            error: None,
        };
        write_rpc_response(&mut stream, &response);
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--json")
        .arg("--timeout")
        .arg("1")
        .output()
        .expect("run rnpath-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["status"].as_str(), Some("found"));
    assert_eq!(value["next_hop"].as_str(), Some("8899aabbccddeeff0011223344556677"));
    server.join().expect("mock rpc server");
}

#[cfg(unix)]
#[test]
fn rnpath_fetches_path_over_unix_rpc() {
    let temp = tempfile::tempdir().expect("temp dir");
    let socket_path = temp.path().join("rnpath.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind mock unix rpc");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept unix rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.starts_with("POST /rpc HTTP/1.1\r\n"), "request: {request_text}");
        assert!(request_text.contains("\r\nHost: localhost\r\n"), "request: {request_text}");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.id, 1);
        assert_eq!(rpc_request.method, "request_path");
        let params = rpc_request.params.expect("params");
        assert_eq!(params["destination_hash"].as_str(), Some("00112233445566778899aabbccddeeff"));
        assert_eq!(params["timeout_secs"].as_u64(), Some(5));
        assert!(params["on_iface"].is_null());
        assert!(params["tag_hex"].is_null());

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "status": "found",
                "requested": true,
                "path_found": true,
                "next_hop": "8899aabbccddeeff0011223344556677",
                "interface": "unix-test",
            })),
            error: None,
        };
        write_rpc_response(&mut stream, &response);
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc-unix")
        .arg(&socket_path)
        .arg("--json")
        .arg("--timeout")
        .arg("5")
        .output()
        .expect("run rnpath-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["status"].as_str(), Some("found"));
    assert_eq!(value["interface"].as_str(), Some("unix-test"));
    server.join().expect("mock unix rpc server");
}

#[cfg(unix)]
#[test]
fn rnpath_rejects_tcp_and_unix_rpc_together() {
    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg("127.0.0.1:4243")
        .arg("--rpc-unix")
        .arg("/tmp/rnpath.sock")
        .output()
        .expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("cannot be used with"), "stderr: {stderr}");
}

#[test]
fn rnpath_rejects_malformed_tag_before_backend_work() {
    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--tag-hex")
        .arg("not-hex")
        .output()
        .expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("tag must be hexadecimal"));
}

#[test]
fn rnpath_times_out_when_daemon_does_not_find_path() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "request_path");
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "status": "timeout",
                "requested": true,
                "path_found": false,
            })),
            error: None,
        }
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("--timeout")
        .arg("1")
        .output()
        .expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("did not complete: timeout"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnpath_read_timeout_has_headroom_for_daemon_path_wait() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "request_path");
        thread::sleep(Duration::from_millis(1_200));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "status": "timeout",
                "requested": true,
                "path_found": false,
            })),
            error: None,
        }
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("--timeout")
        .arg("1")
        .output()
        .expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("did not complete: timeout"),
        "stderr should report daemon timeout result instead of a socket read timeout: {stderr}"
    );
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnpath_reports_daemon_without_path_rpc_without_claiming_success() {
    let rpc = spawn_mock_rpc(|request| RpcResponse {
        id: request.id,
        result: None,
        error: Some(RpcError::new("NOT_IMPLEMENTED", "method not implemented")),
    });

    let output = Command::new(rnpath_bin())
        .arg("00112233445566778899aabbccddeeff")
        .arg("--rpc")
        .arg(rpc.addr)
        .output()
        .expect("run rnpath-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("request_path is not implemented by this daemon"));
    assert!(stderr.contains("rnpath-rs is ready for the daemon path RPC"));
    rpc.thread.join().expect("mock rpc server");
}

fn rnpath_bin() -> String {
    env!("CARGO_BIN_EXE_rnpath-rs").to_string()
}

struct MockRpc {
    addr: String,
    thread: thread::JoinHandle<()>,
}

fn spawn_mock_rpc<F>(handler: F) -> MockRpc
where
    F: FnOnce(rns_rpc::RpcRequest) -> RpcResponse + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let addr = listener.local_addr().expect("mock rpc addr").to_string();
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        let response = handler(rpc_request);
        write_rpc_response(&mut stream, &response);
    });

    MockRpc { addr, thread }
}

fn write_rpc_response(stream: &mut impl Write, response: &RpcResponse) {
    let body = codec::encode_frame(response).expect("encode response");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("write response headers");
    stream.write_all(&body).expect("write response body");
}

fn http_body(request: &[u8]) -> &[u8] {
    let marker = b"\r\n\r\n";
    let start = request
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|index| index + marker.len())
        .expect("http body marker");
    &request[start..]
}
