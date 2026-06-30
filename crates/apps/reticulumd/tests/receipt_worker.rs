use reticulum_daemon::receipt_bridge::{handle_receipt_event, ReceiptEvent};
use rns_rpc::{RpcDaemon, RpcRequest};
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeSet;

fn store_outbound_message(daemon: &RpcDaemon, message_id: &str) {
    let _ = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "send_message".into(),
            params: Some(json!({
                "id": message_id,
                "source": "peer-a",
                "destination": "peer-b",
                "title": "Hi",
                "content": "hello"
            })),
        })
        .unwrap();
}

fn poll_receipt_payload(daemon: &RpcDaemon, request_id: u64) -> JsonValue {
    let poll = daemon
        .handle_rpc(RpcRequest {
            id: request_id,
            method: "sdk_poll_events_v2".into(),
            params: Some(json!({ "cursor": null, "max": 16 })),
        })
        .expect("poll SDK events");
    let result = poll.result.expect("poll result");
    result["events"]
        .as_array()
        .expect("event array")
        .iter()
        .find(|event| event["event_type"] == json!("receipt"))
        .expect("receipt event")
        .get("payload")
        .expect("receipt payload")
        .clone()
}

fn payload_keys(payload: &JsonValue) -> BTreeSet<String> {
    payload.as_object().expect("receipt payload object").keys().cloned().collect()
}

#[test]
fn receipt_event_updates_store_and_emits_event() {
    let daemon = RpcDaemon::test_instance();
    store_outbound_message(&daemon, "msg-1");

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

#[test]
fn transport_receipt_event_matches_rpc_pollable_sdk_payload() {
    let rpc_daemon = RpcDaemon::test_instance();
    store_outbound_message(&rpc_daemon, "msg-rpc");
    rpc_daemon
        .handle_rpc(RpcRequest {
            id: 20,
            method: "record_receipt".into(),
            params: Some(json!({
                "message_id": "msg-rpc",
                "status": "delivered",
            })),
        })
        .expect("record RPC-origin receipt");

    let transport_daemon = RpcDaemon::test_instance();
    store_outbound_message(&transport_daemon, "msg-transport");
    handle_receipt_event(
        &transport_daemon,
        ReceiptEvent { message_id: "msg-transport".into(), status: "delivered".into() },
    )
    .expect("handle transport-origin receipt");

    let rpc_payload = poll_receipt_payload(&rpc_daemon, 21);
    let transport_payload = poll_receipt_payload(&transport_daemon, 22);

    assert_eq!(rpc_payload["message_id"], json!("msg-rpc"));
    assert_eq!(transport_payload["message_id"], json!("msg-transport"));
    assert_eq!(payload_keys(&rpc_payload), payload_keys(&transport_payload));
    assert_eq!(rpc_payload["status"], transport_payload["status"]);
    assert_eq!(rpc_payload["updated"], transport_payload["updated"]);
    assert_eq!(rpc_payload["reason_code"], transport_payload["reason_code"]);
    assert_eq!(transport_payload["status"], json!("delivered"));
    assert_eq!(transport_payload["updated"], json!(true));
    assert!(transport_payload["reason_code"].is_null());
}

#[test]
fn resource_completion_receipt_stays_non_terminal_until_delivery_receipt() {
    let daemon = RpcDaemon::test_instance();
    store_outbound_message(&daemon, "msg-resource-1");

    handle_receipt_event(
        &daemon,
        ReceiptEvent { message_id: "msg-resource-1".into(), status: "sent: link resource".into() },
    )
    .expect("record resource sent");

    let status = daemon
        .handle_rpc(RpcRequest { id: 11, method: "list_messages".into(), params: None })
        .unwrap();
    assert_eq!(
        status.result.unwrap()["messages"][0]["receipt_status"],
        json!("sent: link resource")
    );

    handle_receipt_event(
        &daemon,
        ReceiptEvent { message_id: "msg-resource-1".into(), status: "delivered".into() },
    )
    .expect("record delivered");

    let final_status = daemon
        .handle_rpc(RpcRequest { id: 12, method: "list_messages".into(), params: None })
        .unwrap();
    assert_eq!(final_status.result.unwrap()["messages"][0]["receipt_status"], json!("delivered"));
}
