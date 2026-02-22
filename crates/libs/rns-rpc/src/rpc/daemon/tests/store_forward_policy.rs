#[test]
fn sdk_store_forward_reject_new_blocks_send_when_capacity_reached() {
    let daemon = RpcDaemon::test_instance();

    let configure = daemon
        .handle_rpc(rpc_request(
            900,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 1,
                        "max_message_age_ms": 86_400_000,
                        "capacity_policy": "reject_new",
                        "eviction_priority": "terminal_first"
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());

    let first = daemon
        .handle_rpc(rpc_request(
            901,
            "send_message",
            json!({
                "id": "sf-reject-1",
                "source": "source.a",
                "destination": "destination.a",
                "content": "payload-1"
            }),
        ))
        .expect("first send");
    assert!(first.error.is_none());

    let second = daemon
        .handle_rpc(rpc_request(
            902,
            "send_message",
            json!({
                "id": "sf-reject-2",
                "source": "source.a",
                "destination": "destination.a",
                "content": "payload-2"
            }),
        ))
        .expect("second send");
    assert_eq!(
        second.error.expect("error").code,
        "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED"
    );
}

#[test]
fn sdk_store_forward_drop_oldest_prunes_to_admit_new_message() {
    let daemon = RpcDaemon::test_instance();

    let configure = daemon
        .handle_rpc(rpc_request(
            910,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 1,
                        "max_message_age_ms": 86_400_000,
                        "capacity_policy": "drop_oldest",
                        "eviction_priority": "oldest_first"
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());

    let first = daemon
        .handle_rpc(rpc_request(
            911,
            "send_message",
            json!({
                "id": "sf-drop-1",
                "source": "source.b",
                "destination": "destination.b",
                "content": "payload-1"
            }),
        ))
        .expect("first send");
    assert!(first.error.is_none());

    let second = daemon
        .handle_rpc(rpc_request(
            912,
            "send_message",
            json!({
                "id": "sf-drop-2",
                "source": "source.b",
                "destination": "destination.b",
                "content": "payload-2"
            }),
        ))
        .expect("second send");
    assert!(second.error.is_none());

    assert!(daemon.store.get_message("sf-drop-1").expect("lookup old").is_none());
    assert!(daemon.store.get_message("sf-drop-2").expect("lookup new").is_some());
}

#[test]
fn sdk_store_forward_expiry_marks_old_non_terminal_records() {
    let daemon = RpcDaemon::test_instance();

    let configure = daemon
        .handle_rpc(rpc_request(
            920,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 32,
                        "max_message_age_ms": 5,
                        "capacity_policy": "drop_oldest",
                        "eviction_priority": "terminal_first"
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());

    daemon
        .store
        .insert_message(&MessageRecord {
            id: "sf-old".to_string(),
            source: "source.c".to_string(),
            destination: "destination.c".to_string(),
            title: "".to_string(),
            content: "payload-old".to_string(),
            timestamp: now_i64().saturating_sub(1_000),
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("insert old record");

    let send = daemon
        .handle_rpc(rpc_request(
            921,
            "send_message",
            json!({
                "id": "sf-trigger",
                "source": "source.c",
                "destination": "destination.c",
                "content": "payload-new"
            }),
        ))
        .expect("send trigger");
    assert!(send.error.is_none());

    let old = daemon
        .store
        .get_message("sf-old")
        .expect("lookup old")
        .expect("old record should still exist after expiry marking");
    assert_eq!(old.receipt_status.as_deref(), Some("expired"));
}
