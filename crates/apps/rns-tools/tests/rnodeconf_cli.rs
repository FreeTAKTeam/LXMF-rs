use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::RpcResponse;
use serde_json::json;

#[test]
fn rnodeconf_sends_query_radio_state_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("radio_state_query"));
        assert!(params.get("pattern").is_none());
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "radio_state_query"
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("query-radio-state")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["queued"].as_bool(), Some(true));
    assert_eq!(value["command"].as_str(), Some("radio_state_query"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_blink_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("blink"));
        assert_eq!(params["pattern"].as_u64(), Some(3));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "blink",
                "pattern": 3
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("blink")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--pattern")
        .arg("3")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["queued"].as_bool(), Some(true));
    assert_eq!(value["command"].as_str(), Some("blink"));
    assert_eq!(value["pattern"].as_u64(), Some(3));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_read_config_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("config_read"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "config_read"
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("read-config")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("config_read"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_display_intensity_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("display_intensity"));
        assert_eq!(params["intensity"].as_u64(), Some(8));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "display_intensity",
                "intensity": 8
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("set-display-intensity")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--intensity")
        .arg("8")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("display_intensity"));
    assert_eq!(value["intensity"].as_u64(), Some(8));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_disable_interference_avoidance_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("disable_interference_avoidance"));
        assert_eq!(params["disabled"].as_bool(), Some(true));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "disable_interference_avoidance",
                "disabled": true
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("disable-interference-avoidance")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("disable_interference_avoidance"));
    assert_eq!(value["disabled"].as_bool(), Some(true));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_persistent_rnode_management_guard_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("wifi_psk"));
        assert_eq!(params["confirm_persistent"].as_bool(), Some(true));
        assert_eq!(params["psk"].as_str(), Some("abcdefgh"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "wifi_psk",
                "confirmation": "persistent",
                "psk_set": true
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("set-wifi-psk")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--psk")
        .arg("abcdefgh")
        .arg("--confirm-persistent")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("wifi_psk"));
    assert_eq!(value["psk_set"].as_bool(), Some(true));
    assert!(value.get("psk").is_none());
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_destructive_rnode_management_guard_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("rom_write"));
        assert_eq!(params["address"].as_u64(), Some(9));
        assert_eq!(params["byte"].as_u64(), Some(42));
        assert_eq!(params["confirm_destructive"].as_bool(), Some(true));
        assert_eq!(params["confirm_command"].as_str(), Some("rom_write"));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "rom_write",
                "confirmation": "destructive",
                "address": 9,
                "byte": 42
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("write-rom")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--address")
        .arg("9")
        .arg("--byte")
        .arg("42")
        .arg("--confirm-destructive")
        .arg("--confirm-command")
        .arg("rom_write")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["command"].as_str(), Some("rom_write"));
    assert_eq!(value["confirmation"].as_str(), Some("destructive"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_sends_guarded_rnode_multi_vport_rpc() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "rnode_management");
        let params = request.params.expect("params");
        assert_eq!(params["iface"].as_str(), Some("rnode-main"));
        assert_eq!(params["command"].as_str(), Some("config_save"));
        assert_eq!(params["vport"].as_u64(), Some(2));
        assert_eq!(params["confirm_persistent"].as_bool(), Some(true));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "queued": true,
                "iface": "rnode-main",
                "command": "config_save",
                "vport": 2,
                "confirmation": "persistent"
            })),
            error: None,
        }
    });

    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg(rpc.addr)
        .arg("save-config")
        .arg("--interface")
        .arg("rnode-main")
        .arg("--vport")
        .arg("2")
        .arg("--confirm-persistent")
        .output()
        .expect("run rnodeconf-rs");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["vport"].as_u64(), Some(2));
    assert_eq!(value["confirmation"].as_str(), Some("persistent"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnodeconf_rejects_missing_management_confirmation_before_rpc() {
    let output = Command::new(rnodeconf_bin())
        .arg("--rpc")
        .arg("127.0.0.1:9")
        .arg("save-config")
        .arg("--interface")
        .arg("rnode-main")
        .output()
        .expect("run rnodeconf-rs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("requires --confirm-persistent"), "{stderr}");
    assert!(!stderr.contains("Connection refused"), "{stderr}");
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

fn rnodeconf_bin() -> String {
    env!("CARGO_BIN_EXE_rnodeconf-rs").to_string()
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
