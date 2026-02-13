use serde_json::Value;
use std::path::PathBuf;

fn load_fixture() -> Value {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/fixtures/contract-v2/payload-domains.json");
    let bytes = std::fs::read(&fixture_path).unwrap_or_else(|_| {
        panic!("shared contract fixture should be readable at {}", fixture_path.display())
    });
    serde_json::from_slice(&bytes).expect("shared contract fixture should be valid json")
}

fn section<'a>(root: &'a Value, name: &str) -> &'a serde_json::Map<String, Value> {
    root.get(name)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("fixture section {name} should be an object"))
}

#[test]
fn shared_fixture_includes_all_payload_domains() {
    let root = load_fixture();
    let message_list = section(&root, "message_list");
    let messages = message_list
        .get("messages")
        .and_then(Value::as_array)
        .expect("message_list.messages should be an array");
    let message = messages
        .first()
        .and_then(Value::as_object)
        .expect("message_list.messages[0] should be an object");
    let fields = message
        .get("fields")
        .and_then(Value::as_object)
        .expect("message fields should be an object");

    for field_id in ["2", "5", "9", "12", "14", "16"] {
        assert!(
            fields.contains_key(field_id),
            "message fields must include canonical payload domain field {field_id}"
        );
    }

    for key in ["attachments", "paper", "announce", "peer_snapshot", "interface_snapshot"] {
        assert!(fields.contains_key(key), "message fields must include {key} domain payload");
    }
}

#[test]
fn shared_fixture_includes_transport_and_event_domains() {
    let root = load_fixture();

    let announce = section(&root, "announce_list");
    assert!(
        announce.get("announces").and_then(Value::as_array).is_some_and(|items| !items.is_empty()),
        "announce_list.announces must include at least one record"
    );

    let peers = section(&root, "peer_list");
    assert!(
        peers.get("peers").and_then(Value::as_array).is_some_and(|items| !items.is_empty()),
        "peer_list.peers must include at least one record"
    );

    let interfaces = section(&root, "interface_list");
    assert!(
        interfaces
            .get("interfaces")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty()),
        "interface_list.interfaces must include at least one record"
    );

    let nodes = section(&root, "propagation_node_list");
    assert!(
        nodes.get("nodes").and_then(Value::as_array).is_some_and(|items| !items.is_empty()),
        "propagation_node_list.nodes must include at least one record"
    );

    let trace = section(&root, "message_delivery_trace");
    assert!(
        trace.get("transitions").and_then(Value::as_array).is_some_and(|items| items.len() >= 2),
        "message_delivery_trace.transitions should include retry and terminal states"
    );

    let event = section(&root, "rpc_event");
    assert_eq!(
        event.get("event_type").and_then(Value::as_str),
        Some("announce_received"),
        "rpc_event should include an announce_received event sample"
    );
}
