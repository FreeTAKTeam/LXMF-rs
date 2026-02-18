use reticulum::e2e_harness::build_daemon_args;
use reticulum::e2e_harness::is_ready_line;
use reticulum::e2e_harness::message_present;
use reticulum::e2e_harness::peer_present;
use reticulum::e2e_harness::timestamp_millis;
use reticulum::e2e_harness::{build_http_post, parse_http_response_body};
use reticulum::e2e_harness::{build_rpc_body, parse_rpc_response};
use reticulum::e2e_harness::{build_rpc_frame, parse_rpc_frame};
use reticulum::e2e_harness::{build_send_params, build_tcp_client_config};
use serde_json::json;

#[test]
fn ready_line_detects_daemon_listening() {
    assert!(is_ready_line("reticulumd listening on http://127.0.0.1:4243"));
    assert!(!is_ready_line("starting daemon..."));
}

#[test]
fn rpc_helpers_roundtrip() {
    let body = build_rpc_body(1, "status", None).expect("build rpc body");
    assert!(body.contains("\"method\":\"status\""));

    let resp = json!({"id":1,"result":{"ok":true},"error":null}).to_string();
    let parsed = parse_rpc_response(&resp).expect("parse");
    assert_eq!(parsed.id, 1);
}

#[test]
fn rpc_frame_helpers_roundtrip() {
    let body = build_rpc_frame(7, "status", None).expect("build frame");
    let request: reticulum::rpc::RpcRequest =
        reticulum::rpc::codec::decode_frame(&body).expect("decode frame");
    assert_eq!(request.method, "status");

    let resp =
        reticulum::rpc::RpcResponse { id: 7, result: Some(json!({"ok": true})), error: None };
    let framed = reticulum::rpc::codec::encode_frame(&resp).expect("encode frame");
    let parsed = parse_rpc_frame(&framed).expect("parse frame");
    assert_eq!(parsed.id, 7);
}

#[test]
fn http_post_builder_includes_content_length() {
    let request = build_http_post("/rpc", "127.0.0.1:4243", b"abc");
    let text = String::from_utf8_lossy(&request);
    assert!(text.starts_with("POST /rpc HTTP/1.1"));
    assert!(text.contains("Content-Length: 3"));
    assert!(text.ends_with("\r\n\r\nabc"));
}

#[test]
fn http_response_body_extracts_payload() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
    let body = parse_http_response_body(response).expect("body");
    assert_eq!(body, b"hello");
}

#[test]
fn send_params_include_required_fields() {
    let send = build_send_params("msg-1", "alice", "bob", "hi");
    assert_eq!(send["id"], "msg-1");
    assert_eq!(send["source"], "alice");
    assert_eq!(send["destination"], "bob");
    assert_eq!(send["content"], "hi");
    assert!(send.get("fields").is_some());
}

#[test]
fn tcp_client_config_includes_required_fields() {
    let config = build_tcp_client_config("127.0.0.1", 4242);
    assert!(config.contains("[[interfaces]]"));
    assert!(config.contains("type = \"tcp_client\""));
    assert!(config.contains("enabled = true"));
    assert!(config.contains("host = \"127.0.0.1\""));
    assert!(config.contains("port = 4242"));
}

#[test]
fn message_present_detects_ids() {
    let response = reticulum::rpc::RpcResponse {
        id: 1,
        result: Some(json!({
            "messages": [
                {"id": "one"},
                {"id": "two"}
            ]
        })),
        error: None,
    };
    assert!(message_present(&response, "two"));
    assert!(!message_present(&response, "missing"));
}

#[test]
fn timestamp_millis_is_nonzero() {
    assert!(timestamp_millis() > 0);
}

#[test]
fn peer_present_detects_peer() {
    let response = reticulum::rpc::RpcResponse {
        id: 1,
        result: Some(serde_json::json!({
            "peers": [
                {"peer": "peer-a"},
                {"peer": "peer-b"}
            ]
        })),
        error: None,
    };
    assert!(peer_present(&response, "peer-b"));
    assert!(!peer_present(&response, "peer-missing"));
}

#[test]
fn daemon_args_include_rpc_and_db() {
    let args = build_daemon_args("127.0.0.1:4243", "db.sqlite", 0, None, None);
    assert!(args.contains(&"--rpc".to_string()));
    assert!(args.contains(&"127.0.0.1:4243".to_string()));
    assert!(args.contains(&"--db".to_string()));
    assert!(args.contains(&"db.sqlite".to_string()));
}

#[test]
fn daemon_args_include_transport_when_set() {
    let args = build_daemon_args("127.0.0.1:4243", "db.sqlite", 0, Some("0.0.0.0:4242"), None);
    assert!(args.contains(&"--transport".to_string()));
    assert!(args.contains(&"0.0.0.0:4242".to_string()));
}

#[test]
fn daemon_args_include_config_when_set() {
    let args = build_daemon_args(
        "127.0.0.1:4243",
        "db.sqlite",
        0,
        Some("0.0.0.0:4242"),
        Some("reticulum.toml"),
    );
    assert!(args.contains(&"--config".to_string()));
    assert!(args.contains(&"reticulum.toml".to_string()));
}
