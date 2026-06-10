use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::RpcResponse;
use serde_json::json;

#[test]
fn rnstatus_fetches_daemon_status_and_renders_interface_runtime_state() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let rpc = listener.local_addr().expect("mock rpc addr").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.method, "daemon_status_ex");

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "0123456789abcdef0123456789abcdef",
                "running": true,
                "interface_count": 1,
                "interfaces": [{
                    "name": "field-uplink",
                    "type": "tcp_server",
                    "enabled": true,
                    "host": "0.0.0.0",
                    "port": 4242,
                    "settings": {
                        "_runtime": {
                            "startup_status": "failed",
                            "startup_error": "bind denied"
                        }
                    }
                }]
            })),
            error: None,
        };
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });

    let output = Command::new(rnstatus_bin())
        .arg("--rpc")
        .arg(rpc)
        .arg("--json")
        .output()
        .expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(value["identity_hash"], "0123456789abcdef0123456789abcdef");
    assert_eq!(value["interfaces"][0]["settings"]["_runtime"]["startup_status"], "failed");

    server.join().expect("mock rpc server");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let rpc = listener.local_addr().expect("mock rpc addr").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "0123456789abcdef0123456789abcdef",
                "running": true,
                "interface_count": 1,
                "interfaces": [{
                    "name": "field-uplink",
                    "type": "tcp_server",
                    "enabled": true,
                    "host": "0.0.0.0",
                    "port": 4242,
                    "settings": {
                        "_runtime": {
                            "startup_status": "failed",
                            "startup_error": "bind denied"
                        }
                    }
                }]
            })),
            error: None,
        };
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });

    let output =
        Command::new(rnstatus_bin()).arg("--rpc").arg(rpc).output().expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("field-uplink"), "stdout: {stdout}");
    assert!(stdout.contains("tcp_server"), "stdout: {stdout}");
    assert!(stdout.contains("failed"), "stdout: {stdout}");
    assert!(stdout.contains("bind denied"), "stdout: {stdout}");

    server.join().expect("mock rpc server");
}

fn rnstatus_bin() -> String {
    env!("CARGO_BIN_EXE_rnstatus-rs").to_string()
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
