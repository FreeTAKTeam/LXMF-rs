use std::sync::{Arc, Mutex};

use reticulum::rpc::{OutboundBridge, OutboundDeliveryOptions, RpcDaemon, RpcRequest};
use serde_json::json;

struct TestBridge {
    calls: Arc<Mutex<u32>>,
}

impl OutboundBridge for TestBridge {
    fn deliver(
        &self,
        _record: &reticulum::storage::messages::MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let mut guard = self.calls.lock().expect("calls");
        *guard += 1;
        Ok(())
    }
}

struct FailingBridge;

impl OutboundBridge for FailingBridge {
    fn deliver(
        &self,
        _record: &reticulum::storage::messages::MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        Err(std::io::Error::other("simulated failure"))
    }
}

#[test]
fn send_message_calls_bridge() {
    let calls = Arc::new(Mutex::new(0));
    let bridge = TestBridge { calls: calls.clone() };
    let daemon = RpcDaemon::with_store_and_bridge(
        reticulum::storage::messages::MessagesStore::in_memory().expect("store"),
        "test".into(),
        Arc::new(bridge),
    );

    let request = RpcRequest {
        id: 1,
        method: "send_message".into(),
        params: Some(json!({
            "id": "msg-1",
            "source": "alice",
            "destination": "bob",
            "title": "",
            "content": "hi",
            "fields": null
        })),
    };

    daemon.handle_rpc(request).expect("response");

    let count = *calls.lock().expect("calls");
    assert_eq!(count, 1);
}

#[test]
fn send_message_reports_delivery_failure() {
    let daemon = RpcDaemon::with_store_and_bridge(
        reticulum::storage::messages::MessagesStore::in_memory().expect("store"),
        "test".into(),
        Arc::new(FailingBridge),
    );

    let request = RpcRequest {
        id: 1,
        method: "send_message".into(),
        params: Some(json!({
            "id": "msg-fail",
            "source": "alice",
            "destination": "bob",
            "title": "",
            "content": "hi",
            "fields": null
        })),
    };

    let response = daemon.handle_rpc(request).expect("rpc response");
    let error = response.error.expect("delivery error");
    assert_eq!(error.code, "DELIVERY_FAILED");
    assert!(error.message.contains("simulated failure"));

    let list = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_messages".into(), params: None })
        .expect("list messages");
    let messages = list.result.expect("result")["messages"].as_array().expect("messages").clone();
    assert_eq!(messages.len(), 1);
    assert!(messages[0]["receipt_status"].as_str().unwrap_or_default().starts_with("failed:"));
}
