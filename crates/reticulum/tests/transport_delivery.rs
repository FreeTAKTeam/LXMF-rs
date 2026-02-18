use std::rc::Rc;

use reticulum::rpc::{RpcDaemon, RpcRequest};
use reticulum::transport::test_bridge;
use serde_json::json;

#[test]
fn send_message_emits_inbound_on_peer() {
    let daemon_a = Rc::new(RpcDaemon::test_instance_with_identity("daemon-a"));
    let daemon_b = Rc::new(RpcDaemon::test_instance_with_identity("daemon-b"));

    test_bridge::reset();
    test_bridge::register("daemon-b", daemon_b.clone());

    let _ = daemon_a
        .handle_rpc(RpcRequest {
            id: 1,
            method: "send_message".into(),
            params: Some(json!({
                "id": "msg-1",
                "source": "daemon-a",
                "destination": "daemon-b",
                "content": "hello"
            })),
        })
        .unwrap();

    let event = daemon_b.take_event().expect("inbound event");
    assert_eq!(event.event_type, "inbound");
}
