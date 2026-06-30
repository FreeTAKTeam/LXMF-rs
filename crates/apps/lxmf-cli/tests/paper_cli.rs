use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use rmp_serde::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

#[test]
fn paper_encode_cli_calls_sdk_paper_encode() {
    let rpc = spawn_mock_rpc(vec![start_response(), paper_encode_response()]);

    let output = Command::new(lxmf_bin())
        .arg("--rpc")
        .arg(format!("tcp://{}", rpc.addr))
        .arg("--output")
        .arg("json")
        .arg("paper-encode")
        .arg("--message-id")
        .arg("msg-paper-1")
        .output()
        .expect("run lxmf paper-encode");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: JsonValue = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["ok"].as_bool(), Some(true));
    assert_eq!(value["result"]["envelope"]["uri"].as_str(), Some("lxm://paper/v1/demo"));
    rpc.thread.join().expect("mock rpc server");
}

#[test]
fn paper_decode_cli_calls_sdk_paper_decode_with_uri() {
    let rpc = spawn_mock_rpc(vec![start_response(), paper_decode_response()]);

    let output = Command::new(lxmf_bin())
        .arg("--rpc")
        .arg(format!("tcp://{}", rpc.addr))
        .arg("--output")
        .arg("json")
        .arg("paper-decode")
        .arg("--uri")
        .arg("lxm://paper/v1/demo")
        .arg("--transient-id")
        .arg("transient-cli")
        .arg("--destination-hint")
        .arg("dest-cli")
        .output()
        .expect("run lxmf paper-decode");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: JsonValue = serde_json::from_str(&stdout).expect("json stdout");
    assert_eq!(value["ok"].as_bool(), Some(true));
    assert_eq!(value["result"]["paper"]["transient_id"].as_str(), Some("transient-cli"));
    assert_eq!(value["result"]["paper"]["destination_hint"].as_str(), Some("dest-cli"));
    rpc.thread.join().expect("mock rpc server");
}

fn lxmf_bin() -> String {
    env!("CARGO_BIN_EXE_lxmf").to_string()
}

struct ExpectedResponse {
    method: &'static str,
    check_params: fn(&JsonValue),
    result: JsonValue,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: u64,
    method: String,
    params: Option<JsonValue>,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    id: u64,
    result: Option<JsonValue>,
    error: Option<JsonValue>,
}

struct MockRpc {
    addr: String,
    thread: thread::JoinHandle<()>,
}

fn spawn_mock_rpc(expected: Vec<ExpectedResponse>) -> MockRpc {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let addr = listener.local_addr().expect("mock rpc addr").to_string();
    let thread = thread::spawn(move || {
        for expected in expected {
            let (mut stream, _) = listener.accept().expect("accept rpc request");
            let mut request = Vec::new();
            stream.read_to_end(&mut request).expect("read rpc request");
            let body = http_body(&request);
            let rpc_request = decode_rpc_frame(body).expect("decode request");
            assert_eq!(rpc_request.method, expected.method);
            let params = rpc_request.params.unwrap_or(JsonValue::Null);
            (expected.check_params)(&params);
            let response =
                RpcResponse { id: rpc_request.id, result: Some(expected.result), error: None };
            write_rpc_response(&mut stream, &response);
        }
    });

    MockRpc { addr, thread }
}

fn start_response() -> ExpectedResponse {
    ExpectedResponse {
        method: "sdk_negotiate_v2",
        check_params: |params| {
            assert_eq!(params["supported_contract_versions"][0].as_u64(), Some(2));
        },
        result: json!({
            "runtime_id": "runtime-cli-paper",
            "active_contract_version": 2,
            "effective_capabilities": [
                "sdk.capability.cursor_replay",
                "sdk.capability.async_events",
                "sdk.capability.receipt_terminality",
                "sdk.capability.config_revision_cas",
                "sdk.capability.idempotency_ttl",
                "sdk.capability.paper_messages"
            ],
            "effective_limits": {
                "max_poll_events": 64,
                "max_event_bytes": 32768,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 60000
            },
            "contract_release": "v2",
            "schema_namespace": "sdk.v2",
            "sdk_version": "test",
            "python_reference": {
                "reticulum_conformance_ref": "test",
                "python_reticulum_version": "test",
                "python_reticulum_ref": "test",
                "python_lxmf_version": "test",
                "python_lxmf_ref": "test"
            }
        }),
    }
}

fn paper_encode_response() -> ExpectedResponse {
    ExpectedResponse {
        method: "sdk_paper_encode_v2",
        check_params: |params| {
            assert_eq!(params["message_id"].as_str(), Some("msg-paper-1"));
        },
        result: json!({
            "envelope": {
                "uri": "lxm://paper/v1/demo",
                "transient_id": "transient-cli",
                "destination_hint": "dest-cli"
            }
        }),
    }
}

fn paper_decode_response() -> ExpectedResponse {
    ExpectedResponse {
        method: "sdk_paper_decode_v2",
        check_params: |params| {
            assert_eq!(params["uri"].as_str(), Some("lxm://paper/v1/demo"));
            assert_eq!(params["transient_id"].as_str(), Some("transient-cli"));
            assert_eq!(params["destination_hint"].as_str(), Some("dest-cli"));
        },
        result: json!({
            "paper": {
                "accepted": true,
                "transient_id": "transient-cli",
                "duplicate": false,
                "destination": "dest-cli",
                "destination_hint": "dest-cli",
                "bytes_len": 17,
                "revision": 8
            }
        }),
    }
}

fn write_rpc_response(stream: &mut impl Write, response: &RpcResponse) {
    let body = encode_rpc_frame(response).expect("encode response");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("write response headers");
    stream.write_all(&body).expect("write response body");
}

fn decode_rpc_frame(bytes: &[u8]) -> Result<RpcRequest, Box<dyn std::error::Error>> {
    let payload = rpc_payload(bytes)?;
    Ok(rmp_serde::from_slice(payload)?)
}

fn encode_rpc_frame(response: &RpcResponse) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut frame = Vec::with_capacity(512);
    frame.extend_from_slice(&[0u8; 4]);
    response.serialize(&mut Serializer::new(&mut frame))?;
    let len = u32::try_from(frame.len() - 4)?;
    frame[..4].copy_from_slice(&len.to_be_bytes());
    Ok(frame)
}

fn rpc_payload(bytes: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    let header: [u8; 4] = bytes.get(..4).ok_or("missing rpc frame header")?.try_into()?;
    let len = u32::from_be_bytes(header) as usize;
    let end = 4usize.checked_add(len).ok_or("rpc frame length overflow")?;
    Ok(bytes.get(4..end).ok_or("incomplete rpc frame")?)
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
