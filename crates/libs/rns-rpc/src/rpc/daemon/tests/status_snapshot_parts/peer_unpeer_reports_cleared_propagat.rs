#[test]
fn peer_unpeer_reports_cleared_propagation_queue_accounting() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": "peer-unpeer-accounting" })))
        .expect("sync peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let handled = PropagationEntryRecord {
        transient_id: "c8".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(12),
        received_at: 1_700_000_701,
        size_bytes: 12,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "c9".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(24),
        received_at: 1_700_000_702,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon
        .store
        .mark_peer_handled_propagation("peer-unpeer-accounting", handled.transient_id.as_str())
        .expect("mark handled");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unpeer-accounting", unhandled.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-unpeer-accounting-in".to_string(),
            source: "peer-unpeer-accounting".to_string(),
            destination: "local".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_703,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store inbound message");
    daemon
        .accept_inbound(MessageRecord {
            id: "peer-unpeer-accounting-out".to_string(),
            source: "local".to_string(),
            destination: "peer-unpeer-accounting".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp: 1_700_000_704,
            direction: "out".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("store outbound message");

    let result = daemon
        .handle_rpc(rpc_request(
            94,
            "peer_unpeer",
            json!({ "peer": "peer-unpeer-accounting" }),
        ))
        .expect("unpeer")
        .result
        .expect("unpeer result");
    assert_eq!(result["peer"].as_str(), Some("peer-unpeer-accounting"));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(2));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(36));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(2));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(12));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(result["offered"].as_u64(), Some(2));
    assert_eq!(result["outgoing"].as_u64(), Some(1));
    assert_eq!(result["incoming"].as_u64(), Some(1));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(2));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(36));
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(2));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(12));
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(event.payload["offered"].as_u64(), Some(2));
    assert_eq!(event.payload["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["incoming"].as_u64(), Some(1));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
}

#[test]
fn peer_unpeer_reports_case_variant_live_queue_accounting_like_python() {
    let daemon = RpcDaemon::test_instance();
    let stored_peer = "Peer-Unpeer-Accounting-Case";
    let request_peer = stored_peer.to_ascii_lowercase();
    daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": stored_peer })))
        .expect("sync peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let handled = PropagationEntryRecord {
        transient_id: "e7".repeat(32),
        destination: "35".repeat(16),
        payload_hex: "35".repeat(12),
        received_at: 1_700_000_707,
        size_bytes: 12,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "e8".repeat(32),
        destination: "36".repeat(16),
        payload_hex: "36".repeat(24),
        received_at: 1_700_000_708,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&handled).expect("store handled entry");
    daemon.store.upsert_propagation_entry(&unhandled).expect("store unhandled entry");
    daemon
        .store
        .mark_peer_handled_propagation(request_peer.as_str(), handled.transient_id.as_str())
        .expect("mark case-variant handled");
    daemon
        .store
        .mark_peer_unhandled_propagation(request_peer.as_str(), unhandled.transient_id.as_str())
        .expect("mark case-variant unhandled");

    let result = daemon
        .handle_rpc(rpc_request(94, "peer_unpeer", json!({ "peer": stored_peer })))
        .expect("unpeer")
        .result
        .expect("unpeer result");
    assert_eq!(result["peer"].as_str(), Some(stored_peer));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(2));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(36));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(12));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(24));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(2));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(36));
    assert_eq!(
        event.payload["messages"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(handled.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["messages"]["unhandled_ids"].as_array().expect("event unhandled ids"),
        &[json!(unhandled.transient_id.as_str())]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids(request_peer.as_str())
            .expect("case-variant handled after unpeer")
            .is_empty()
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(request_peer.as_str())
            .expect("case-variant unhandled after unpeer")
            .is_empty()
    );
}

#[test]
fn peer_unpeer_counts_received_and_transfer_limited_queue_marks_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": "peer-unpeer-all-marks" })))
        .expect("sync peer");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let handled = PropagationEntryRecord {
        transient_id: "da".repeat(32),
        destination: "31".repeat(16),
        payload_hex: "31".repeat(10),
        received_at: 1_700_000_703,
        size_bytes: 10,
        stamp_value: None,
    };
    let unhandled = PropagationEntryRecord {
        transient_id: "db".repeat(32),
        destination: "32".repeat(16),
        payload_hex: "32".repeat(20),
        received_at: 1_700_000_704,
        size_bytes: 20,
        stamp_value: None,
    };
    let received = PropagationEntryRecord {
        transient_id: "dc".repeat(32),
        destination: "33".repeat(16),
        payload_hex: "33".repeat(30),
        received_at: 1_700_000_705,
        size_bytes: 30,
        stamp_value: None,
    };
    let transfer_limited = PropagationEntryRecord {
        transient_id: "dd".repeat(32),
        destination: "34".repeat(16),
        payload_hex: "34".repeat(40),
        received_at: 1_700_000_706,
        size_bytes: 40,
        stamp_value: None,
    };
    for entry in [&handled, &unhandled, &received, &transfer_limited] {
        daemon.store.upsert_propagation_entry(entry).expect("store entry");
    }
    daemon
        .store
        .mark_peer_handled_propagation("peer-unpeer-all-marks", handled.transient_id.as_str())
        .expect("mark handled");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-unpeer-all-marks", unhandled.transient_id.as_str())
        .expect("mark unhandled");
    daemon
        .store
        .mark_peer_received_propagation("peer-unpeer-all-marks", received.transient_id.as_str())
        .expect("mark received");
    daemon
        .store
        .mark_peer_transfer_limited_propagation(
            "peer-unpeer-all-marks",
            transfer_limited.transient_id.as_str(),
        )
        .expect("mark transfer limited");

    let result = daemon
        .handle_rpc(rpc_request(
            94,
            "peer_unpeer",
            json!({ "peer": "peer-unpeer-all-marks" }),
        ))
        .expect("unpeer")
        .result
        .expect("unpeer result");
    assert_eq!(result["propagation_cleared"].as_u64(), Some(4));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[
            json!(handled.transient_id.as_str()),
            json!(received.transient_id.as_str()),
            json!(transfer_limited.transient_id.as_str()),
        ]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-unpeer-all-marks")
            .expect("remaining handled ids"),
        Vec::<String>::new()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-unpeer-all-marks")
            .expect("remaining unhandled entries"),
        Vec::<PropagationEntryRecord>::new()
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(4));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(100));
}

#[test]
fn peer_sync_rejects_blank_peer_identifier() {
    let daemon = RpcDaemon::test_instance();

    let err = daemon
        .handle_rpc(rpc_request(94, "peer_sync", json!({ "peer": "   " })))
        .expect_err("blank peer id should be rejected");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("peer is required"));
}

#[test]
fn lxmf_metadata_entries_merge_without_changing_receipt_status() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "metadata-message".to_string(),
            source: "source".to_string(),
            destination: "destination".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "app": "value",
                "_lxmf": {
                    "existing": true,
                },
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("insert message");

    daemon
        .record_message_lxmf_metadata_entries(
            "metadata-message",
            [
                ("propagation_packed".to_string(), json!(true)),
                ("propagation_packed_size".to_string(), json!(1234)),
                ("propagation_stamp_value".to_string(), json!(19)),
            ],
        )
        .expect("record metadata");

    let result = daemon
        .handle_rpc(RpcRequest { id: 91, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("list messages result");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some("metadata-message"))
        .expect("metadata message");

    assert_eq!(message["receipt_status"].as_str(), Some("sending"));
    assert_eq!(message["fields"]["app"].as_str(), Some("value"));
    assert_eq!(message["fields"]["_lxmf"]["existing"].as_bool(), Some(true));
    assert_eq!(message["fields"]["_lxmf"]["propagation_packed"].as_bool(), Some(true));
    assert_eq!(message["fields"]["_lxmf"]["propagation_packed_size"].as_u64(), Some(1234));
    assert_eq!(message["fields"]["_lxmf"]["propagation_stamp_value"].as_u64(), Some(19));
}
