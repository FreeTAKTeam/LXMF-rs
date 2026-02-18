use reticulum::rpc::{
    codec::{decode_frame, encode_frame},
    RpcDaemon, RpcEvent, RpcRequest, RpcResponse,
};
use reticulum::storage::messages::MessagesStore;

#[test]
fn rpc_http_roundtrip() {
    let store = MessagesStore::in_memory().unwrap();
    let daemon = RpcDaemon::with_store(store, "daemon".into());

    let req = RpcRequest { id: 1, method: "status".into(), params: None };
    let framed = encode_frame(&req).unwrap();

    let mut request_bytes = Vec::new();
    request_bytes.extend_from_slice(b"POST /rpc HTTP/1.1\r\n");
    request_bytes.extend_from_slice(b"Host: localhost\r\n");
    request_bytes.extend_from_slice(b"Content-Type: application/msgpack\r\n");
    request_bytes.extend_from_slice(format!("Content-Length: {}\r\n", framed.len()).as_bytes());
    request_bytes.extend_from_slice(b"\r\n");
    request_bytes.extend_from_slice(&framed);

    let response = reticulum::rpc::http::handle_http_request(&daemon, &request_bytes).unwrap();
    let body_start = response.windows(4).position(|window| window == b"\r\n\r\n").unwrap() + 4;
    let resp: RpcResponse = decode_frame(&response[body_start..]).unwrap();
    assert_eq!(resp.id, 1);
}

#[test]
fn rpc_http_events_returns_inbound() {
    let store = MessagesStore::in_memory().unwrap();
    let daemon = RpcDaemon::with_store(store, "daemon".into());
    daemon.inject_inbound_test_message("hello");

    let request_bytes = b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
    let response = reticulum::rpc::http::handle_http_request(&daemon, &request_bytes).unwrap();
    let body_start = response.windows(4).position(|window| window == b"\r\n\r\n").unwrap() + 4;
    let event: RpcEvent = decode_frame(&response[body_start..]).unwrap();
    assert_eq!(event.event_type, "inbound");
}

#[test]
fn rpc_http_events_drains_queue() {
    let store = MessagesStore::in_memory().unwrap();
    let daemon = RpcDaemon::with_store(store, "daemon".into());
    daemon
        .push_event(RpcEvent { event_type: "one".into(), payload: serde_json::json!({ "i": 1 }) });

    let request_bytes = b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
    let response = reticulum::rpc::http::handle_http_request(&daemon, &request_bytes).unwrap();
    let body_start = response.windows(4).position(|window| window == b"\r\n\r\n").unwrap() + 4;
    let event: RpcEvent = decode_frame(&response[body_start..]).unwrap();
    assert_eq!(event.event_type, "one");
    assert!(daemon.take_event().is_none());
}
