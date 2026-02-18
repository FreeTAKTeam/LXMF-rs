use reticulum::rpc::{RpcDaemon, RpcRequest};
use serde_json::json;

#[test]
fn receipt_status_roundtrip() {
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

    let _ = daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "record_receipt".into(),
            params: Some(json!({"message_id": "msg-1", "status": "delivered"})),
        })
        .unwrap();

    let list = daemon
        .handle_rpc(RpcRequest { id: 3, method: "list_messages".into(), params: None })
        .unwrap();

    let result = list.result.unwrap();
    let messages = result.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages[0].get("receipt_status").unwrap(), "delivered");
}
