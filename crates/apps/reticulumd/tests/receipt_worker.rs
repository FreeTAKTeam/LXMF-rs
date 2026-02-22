use reticulum_daemon::receipt_bridge::{handle_receipt_event, ReceiptEvent};
use rns_rpc::{RpcDaemon, RpcRequest};
use serde_json::json;

#[test]
fn receipt_event_updates_store_and_emits_event() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "send_message".into(),
            params: Some(json!({
                "id": "msg-1",
                "source": "peer-a",
                "destination": "peer-b",
                "title": "Hi",
                "content": "hello"
            })),
        })
        .unwrap();

    handle_receipt_event(
        &daemon,
        ReceiptEvent { message_id: "msg-1".into(), status: "delivered".into() },
    )
    .expect("handle receipt");

    let list = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_messages".into(), params: None })
        .unwrap();

    let result = list.result.unwrap();
    let messages = result.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages[0].get("receipt_status").unwrap(), "delivered");
}
