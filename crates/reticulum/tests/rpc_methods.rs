use reticulum::rpc::{RpcDaemon, RpcRequest};
use reticulum::storage::messages::MessagesStore;

#[test]
fn status_returns_identity() {
    let daemon = RpcDaemon::test_instance();
    let resp =
        daemon.handle_rpc(RpcRequest { id: 1, method: "status".into(), params: None }).unwrap();
    let result = resp.result.unwrap();
    assert!(result["identity_hash"].is_string());
    assert!(result["delivery_destination_hash"].is_string());
}

#[test]
fn status_uses_custom_identity() {
    let store = MessagesStore::in_memory().unwrap();
    let daemon = RpcDaemon::with_store(store, "daemon-identity".into());
    let resp =
        daemon.handle_rpc(RpcRequest { id: 1, method: "status".into(), params: None }).unwrap();
    let result = resp.result.unwrap();
    assert_eq!(result["identity_hash"], "daemon-identity");
    assert_eq!(result["delivery_destination_hash"], "daemon-identity");
}

#[test]
fn status_prefers_configured_delivery_destination_hash() {
    let store = MessagesStore::in_memory().unwrap();
    let daemon = RpcDaemon::with_store(store, "daemon-identity".into());
    daemon.set_delivery_destination_hash(Some("delivery-hash".into()));
    let resp =
        daemon.handle_rpc(RpcRequest { id: 1, method: "status".into(), params: None }).unwrap();
    let result = resp.result.unwrap();
    assert_eq!(result["identity_hash"], "daemon-identity");
    assert_eq!(result["delivery_destination_hash"], "delivery-hash");
}

#[test]
fn send_message_persists() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "send_message".into(),
            params: Some(serde_json::json!({
                "id": "msg-1",
                "source": "alice",
                "destination": "bob",
                "content": "hello"
            })),
        })
        .unwrap();

    let resp = daemon
        .handle_rpc(RpcRequest { id: 3, method: "list_messages".into(), params: None })
        .unwrap();

    let result = resp.result.unwrap();
    assert_eq!(result["meta"]["contract_version"], "v2");
    let items = result["messages"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);
}

#[test]
fn receive_message_persists_and_emits_event() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 5,
            method: "receive_message".into(),
            params: Some(serde_json::json!({
                "id": "msg-2",
                "source": "alice",
                "destination": "bob",
                "content": "hello"
            })),
        })
        .unwrap();

    let resp = daemon
        .handle_rpc(RpcRequest { id: 6, method: "list_messages".into(), params: None })
        .unwrap();

    let result = resp.result.unwrap();
    assert_eq!(result["meta"]["contract_version"], "v2");
    let items = result["messages"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);

    let event = daemon.take_event().expect("event");
    assert_eq!(event.event_type, "inbound");
    assert_eq!(event.payload["message"]["id"], "msg-2");
}

#[test]
fn list_peers_returns_empty_array() {
    let daemon = RpcDaemon::test_instance();
    let resp =
        daemon.handle_rpc(RpcRequest { id: 4, method: "list_peers".into(), params: None }).unwrap();

    let result = resp.result.unwrap();
    assert_eq!(result["meta"]["contract_version"], "v2");
    let peers = result["peers"].as_array().unwrap().clone();
    assert_eq!(peers.len(), 0);
}
