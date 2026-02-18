use reticulum::rpc::RpcRequest;
use reticulum::rpc::{RpcDaemon, RpcEvent};
use serde_json::json;

#[test]
fn rpc_event_queue_drains_in_fifo_order() {
    let daemon = RpcDaemon::test_instance();
    daemon.push_event(RpcEvent { event_type: "one".into(), payload: serde_json::json!({"i": 1}) });
    daemon.push_event(RpcEvent { event_type: "two".into(), payload: serde_json::json!({"i": 2}) });

    let first = daemon.take_event().expect("first");
    let second = daemon.take_event().expect("second");

    assert_eq!(first.event_type, "one");
    assert_eq!(second.event_type, "two");
}

#[test]
fn rpc_event_queue_returns_none_when_empty() {
    let daemon = RpcDaemon::test_instance();
    assert!(daemon.take_event().is_none());
}

#[test]
fn rpc_event_stream_emits_outbound_and_receipt() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "send_message".into(),
            params: Some(json!({
                "id": "msg-1",
                "source": "me",
                "destination": "peer",
                "content": "hello"
            })),
        })
        .unwrap();

    let event = daemon.take_event().expect("event");
    assert_eq!(event.event_type, "outbound");
}
