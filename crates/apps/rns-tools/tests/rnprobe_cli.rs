use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::{RpcRequest, RpcResponse};
use serde_json::json;

#[test]
fn rnprobe_help_exposes_reference_probe_options() {
    let output = Command::new(rnprobe_bin()).arg("--help").output().expect("run rnprobe help");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Send packet probes to a Reticulum destination"));
    assert!(stdout.contains("FULL_NAME"));
    assert!(stdout.contains("DESTINATION_HASH"));
    assert!(stdout.contains("--size"));
    assert!(stdout.contains("--probes"));
    assert!(stdout.contains("--timeout"));
    assert!(stdout.contains("--wait"));
    assert!(stdout.contains("--verbose"));
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("--rpc-unix"));
}

#[test]
fn rnprobe_sends_packet_probe_options_and_renders_human_summary() {
    let rpc = spawn_mock_rpc(|request| {
        assert_eq!(request.method, "probe");
        let params = request.params.expect("probe params");
        assert_eq!(params["destination"].as_str(), Some("aabbccddeeff00112233445566778899"));
        assert_eq!(params["full_name"].as_str(), Some("rnstransport.probe"));
        assert_eq!(params["size"].as_u64(), Some(32));
        assert_eq!(params["probes"].as_u64(), Some(2));
        assert_eq!(params["timeout_secs"].as_f64(), Some(1.5));
        assert_eq!(params["wait_secs"].as_f64(), Some(0.25));
        RpcResponse {
            id: request.id,
            result: Some(json!({
                "full_name": "rnstransport.probe",
                "destination": "aabbccddeeff00112233445566778899",
                "size": 32,
                "probes": 2,
                "sent": 2,
                "replies": 2,
                "packet_loss_percent": 0.0,
                "results": [
                    {
                        "probe": 1,
                        "status": "delivered",
                        "rtt_ms": 12.3456,
                        "packet_hash": "11".repeat(32),
                        "hops": 1,
                    },
                    {
                        "probe": 2,
                        "status": "delivered",
                        "rtt_ms": 1001.0,
                        "packet_hash": "22".repeat(32),
                        "hops": 1,
                    },
                ],
            })),
            error: None,
        }
    });

    let output = Command::new(rnprobe_bin())
        .args([
            "rnstransport.probe",
            "AABBCCDDEEFF00112233445566778899",
            "--rpc",
            &rpc.addr,
            "--size",
            "32",
            "--probes",
            "2",
            "--timeout",
            "1.5",
            "--wait",
            "0.25",
            "--verbose",
        ])
        .output()
        .expect("run rnprobe");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Probe: rnstransport.probe"));
    assert!(stdout.contains("1: delivered, 12.346 milliseconds, hops=1"));
    assert!(stdout.contains("2: delivered, 1.001 seconds, hops=1"));
    assert!(stdout.contains("packet_hash="));
    assert!(stdout.contains("Replies: 2/2, packet loss: 0.0%"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnprobe_returns_packet_loss_exit_status_and_json_result() {
    let rpc = spawn_mock_rpc(|request| RpcResponse {
        id: request.id,
        result: Some(json!({
            "full_name": request.params.as_ref().and_then(|params| params["full_name"].as_str()),
            "destination": "00112233445566778899aabbccddeeff",
            "size": 16,
            "probes": 2,
            "sent": 2,
            "replies": 1,
            "packet_loss_percent": 50.0,
            "results": [
                { "probe": 1, "status": "delivered", "rtt_ms": 5.0, "hops": 1 },
                { "probe": 2, "status": "timeout", "packet_hash": "33".repeat(32), "hops": 1 },
            ],
        })),
        error: None,
    });

    let output = Command::new(rnprobe_bin())
        .args([
            "rnstransport.probe",
            "00112233445566778899aabbccddeeff",
            "--rpc",
            &rpc.addr,
            "--probes",
            "2",
            "--json",
        ])
        .output()
        .expect("run rnprobe");

    assert_eq!(output.status.code(), Some(2));
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json probe result");
    assert_eq!(result["replies"].as_u64(), Some(1));
    assert_eq!(result["packet_loss_percent"].as_f64(), Some(50.0));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn rnprobe_rejects_malformed_destination_before_backend_work() {
    let output = Command::new(rnprobe_bin())
        .args(["rnstransport.probe", "not-a-destination"])
        .output()
        .expect("run rnprobe");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("destination hash must be 32 hexadecimal characters"));
}

fn rnprobe_bin() -> String {
    env!("CARGO_BIN_EXE_rnprobe").to_string()
}

struct MockRpc {
    addr: String,
    thread: thread::JoinHandle<()>,
}

fn spawn_mock_rpc<F>(handler: F) -> MockRpc
where
    F: FnOnce(RpcRequest) -> RpcResponse + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let addr = listener.local_addr().expect("mock rpc addr").to_string();
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<RpcRequest>(body).expect("decode request");
        let response = handler(rpc_request);
        let body = codec::encode_frame(&response).expect("encode response");
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
    });

    MockRpc { addr, thread }
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
