use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::{RpcError, RpcRequest, RpcResponse};
use serde_json::json;

fn serve_once(
    response: impl FnOnce(RpcRequest) -> RpcResponse + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock RPC");
    let address = listener.local_addr().expect("RPC address").to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept RPC");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read HTTP request");
        let body_at =
            request.windows(4).position(|part| part == b"\r\n\r\n").expect("HTTP body") + 4;
        let rpc = codec::decode_frame::<RpcRequest>(&request[body_at..]).expect("decode RPC");
        let encoded = codec::encode_frame(&response(rpc)).expect("encode RPC response");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            encoded.len()
        )
        .expect("write HTTP headers");
        stream.write_all(&encoded).expect("write RPC response");
    });
    (address, handle)
}

#[test]
fn named_management_cli_sends_each_operation_and_reports_completion() {
    for (flag, operation) in
        [("--attach", "attach"), ("--detach", "detach"), ("--reload", "reload")]
    {
        let (address, server) = serve_once(move |request| {
            assert_eq!(request.method, "manage_interface");
            assert_eq!(request.params, Some(json!({ "operation": operation, "name": "uplink" })));
            RpcResponse {
                id: request.id,
                result: Some(json!({ "complete": true, "state": "active" })),
                error: None,
            }
        });
        let output = Command::new(env!("CARGO_BIN_EXE_rnstatus-rs"))
            .args(["--rpc", &address, flag, "uplink", "--json"])
            .output()
            .expect("run management CLI");
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("\"complete\": true"));
        server.join().expect("mock RPC server");
    }
}

#[test]
fn named_management_cli_returns_failure_for_incomplete_or_error_response() {
    for rpc_error in [None, Some(RpcError::new("CONFIG_INTERFACE_APPLY_FAILED", "bind failed"))] {
        let (address, server) = serve_once(move |request| RpcResponse {
            id: request.id,
            result: rpc_error.is_none().then(|| json!({ "complete": false })),
            error: rpc_error,
        });
        let output = Command::new(env!("CARGO_BIN_EXE_rnstatus-rs"))
            .args(["--rpc", &address, "--attach", "uplink"])
            .output()
            .expect("run management CLI");
        assert!(!output.status.success());
        server.join().expect("mock RPC server");
    }
}

#[test]
fn named_management_cli_rejects_multiple_operations() {
    let output = Command::new(env!("CARGO_BIN_EXE_rnstatus-rs"))
        .args(["--attach", "one", "--reload", "two"])
        .output()
        .expect("run CLI argument parser");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));

    for conflicting in ["--discovered", "--sort", "--weave-display"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rnstatus-rs"));
        command.args(["--attach", "one", conflicting]);
        if conflicting != "--discovered" {
            command.arg("gravity");
        }
        let output = command.output().expect("run CLI argument parser");
        assert!(!output.status.success(), "{conflicting} must not be silently ignored");
    }
}
