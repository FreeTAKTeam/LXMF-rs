use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::RpcResponse;
use serde_json::json;

#[test]
fn weaveconf_sends_enable_remote_display_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "weave_remote_display_control");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("weave-main"));
        assert_eq!(params["enable"].as_bool(), Some(true));
        assert_eq!(params["remote_switch_id_hex"].as_str(), Some("10203040"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "weave-main",
                "enable": true,
                "remote_switch_id_hex": "10203040"
            })),
            error: None,
        }
    });

    let output = Command::new(weaveconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("enable-remote-display")
        .arg("--interface")
        .arg("weave-main")
        .arg("--remote-switch-id-hex")
        .arg("10203040")
        .output()
        .expect("run weaveconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["queued"].as_bool(), Some(true));
    assert_eq!(value["enable"].as_bool(), Some(true));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn weaveconf_sends_disable_remote_display_rpc_without_switch_override() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "weave_remote_display_control");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("weave-main"));
        assert_eq!(params["enable"].as_bool(), Some(false));
        assert!(params.get("remote_switch_id_hex").is_none());
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "weave-main",
                "enable": false
            })),
            error: None,
        }
    });

    let output = Command::new(weaveconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("disable-remote-display")
        .arg("--interface")
        .arg("weave-main")
        .output()
        .expect("run weaveconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["queued"].as_bool(), Some(true));
    assert_eq!(value["enable"].as_bool(), Some(false));
    rpc.thread.join().expect("mock rpc server");
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
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });
    MockRpc { addr, thread }
}

fn weaveconf_bin() -> String {
    env!("CARGO_BIN_EXE_weaveconf-rs").to_string()
}

fn http_body(request: &[u8]) -> &[u8] {
    let marker = b"\r\n\r\n";
    let start = request
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|index| index + marker.len())
        .expect("request headers");
    &request[start..]
}
