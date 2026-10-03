use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::RpcResponse;
use serde_json::json;

fn run_discovery(args: &[&str], json_output: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock RPC");
    let rpc = listener.local_addr().expect("RPC address").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept RPC");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read RPC");
        let body_start =
            request.windows(4).position(|window| window == b"\r\n\r\n").expect("HTTP body") + 4;
        let request =
            codec::decode_frame::<rns_rpc::RpcRequest>(&request[body_start..]).expect("RPC frame");
        assert_eq!(request.method, "discovered_interfaces");
        let response = RpcResponse {
            id: request.id,
            result: Some(json!([
                {"name": "verified", "type": "BackboneInterface", "status": "available", "impl_name": "RNS", "version": "1.5.5", "value": 16, "last_heard": 1},
                {"name": "stale", "type": "BackboneInterface", "status": "stale", "impl_name": "RNS", "version": "1.5.5", "value": 16, "last_heard": 1},
                {"name": "unverified", "type": "BackboneInterface", "status": "available", "impl_name": null, "version": null, "value": 16, "last_heard": 1}
            ])),
            error: None,
        };
        let body = codec::encode_frame(&response).expect("encode RPC");
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).expect("write header");
        stream.write_all(&body).expect("write body");
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_rnstatus-rs"));
    command.args(["--rpc", &rpc, "--discovered"]);
    if json_output {
        command.arg("--json");
    }
    let output = command.args(args).output().expect("run rnstatus-rs");
    server.join().expect("join RPC server");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("UTF-8 discovery output")
}

#[test]
fn rns_1_5_5_discovery_hides_stale_and_unverified_by_default() {
    let output = run_discovery(&[], false);
    assert!(output.contains("verified"), "output: {output}");
    assert!(!output.contains("unverified"), "output: {output}");
    assert!(!output.contains("stale"), "output: {output}");
}

#[test]
fn rns_1_5_5_discovery_can_show_stale_and_unverified_explicitly() {
    let output = run_discovery(&["--show-stale", "--show-unknown"], false);
    assert!(output.contains("verified"), "output: {output}");
    assert!(output.contains("unverified"), "output: {output}");
    assert!(output.contains("stale"), "output: {output}");
}

#[test]
fn rns_1_5_5_discovery_json_keeps_all_rows_like_python() {
    let output = run_discovery(&[], true);
    let rows: serde_json::Value = serde_json::from_str(&output).expect("JSON discovery output");
    assert_eq!(rows.as_array().expect("rows").len(), 3);
}
