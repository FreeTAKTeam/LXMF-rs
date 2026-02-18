use reticulum::rpc::{RpcDaemon, RpcRequest};
use serde_json::json;

#[test]
fn receive_message_persists_fields() {
    let daemon = RpcDaemon::test_instance();
    let resp = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "receive_message".into(),
            params: Some(json!({
                "id": "msg-1",
                "source": "peer-a",
                "destination": "peer-b",
                "content": "hello",
                "fields": {"k": "v"}
            })),
        })
        .unwrap();
    assert!(resp.error.is_none());

    let list = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_messages".into(), params: None })
        .unwrap();

    let result = list.result.unwrap();
    let messages = result.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages[0].get("fields").unwrap().get("k").unwrap(), "v");
}

#[test]
fn receive_message_persists_title() {
    let daemon = RpcDaemon::test_instance();
    let resp = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "receive_message".into(),
            params: Some(json!({
                "id": "msg-2",
                "source": "peer-a",
                "destination": "peer-b",
                "title": "Hello",
                "content": "body",
            })),
        })
        .unwrap();
    assert!(resp.error.is_none());

    let list = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_messages".into(), params: None })
        .unwrap();

    let result = list.result.unwrap();
    let messages = result.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages[0].get("title").unwrap(), "Hello");
}
