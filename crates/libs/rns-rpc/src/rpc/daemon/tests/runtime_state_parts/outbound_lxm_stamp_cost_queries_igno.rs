#[test]
fn outbound_lxm_stamp_cost_queries_ignore_stale_cost_after_terminal_stamp_state() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, lxmf, method, result_key) in [
        (
            "failed-stamp-state-cost-query",
            json!({ "stamp_state": " FAILED ", "stamp_target_cost": 7 }),
            "get_outbound_lxm_stamp_cost",
            "stamp_cost",
        ),
        (
            "cancelled-propagation-stamp-state-cost-query",
            json!({
                "propagation_stamp_state": " cancelled ",
                "propagation_stamp_target_cost": 9
            }),
            "get_outbound_lxm_propagation_stamp_cost",
            "propagation_stamp_cost",
        ),
    ] {
        daemon
            .accept_inbound(MessageRecord {
                id: message_id.to_string(),
                source: "src".to_string(),
                destination: "dst".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                timestamp: 1_700_000_000,
                direction: "out".to_string(),
                fields: Some(json!({ "_lxmf": lxmf })),
                receipt_status: Some("sending".to_string()),
            })
            .expect("store outbound");

        let stamp_cost = daemon
            .handle_rpc(rpc_request(22, method, json!({ "message_id": message_id })))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert_eq!(stamp_cost[result_key], JsonValue::Null);
    }
}

#[test]
fn paper_encode_marks_message_as_sent_paper() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "paper-node".to_string(),
        Some(Arc::new(PendingOutboundBridge)),
        None,
    );

    let send = daemon
        .handle_rpc(rpc_request(
            12,
            "send_message_v2",
            json!({
                "id": "paper-pending-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");
    assert!(send.error.is_none());

    let encode = daemon
        .handle_rpc(rpc_request(
            13,
            "sdk_paper_encode_v2",
            json!({ "message_id": "paper-pending-1" }),
        ))
        .expect("paper encode");
    assert!(encode.error.is_none());

    let status = daemon
        .handle_rpc(rpc_request(14, "sdk_status_v2", json!({ "message_id": "paper-pending-1" })))
        .expect("status");
    assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("sent: paper"));
}

#[test]
fn sdk_status_v2_returns_message_record() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon
        .handle_rpc(rpc_request(
            40,
            "send_message_v2",
            json!({
                "id": "status-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");
    let response = daemon
        .handle_rpc(rpc_request(
            41,
            "sdk_status_v2",
            json!({
                "message_id": "status-1"
            }),
        ))
        .expect("status");
    assert_eq!(response.result.expect("result")["message"]["id"], json!("status-1"));
}

#[test]
fn sdk_property_terminal_receipt_status_is_sticky() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon
        .handle_rpc(rpc_request(
            45,
            "send_message_v2",
            json!({
                "id": "property-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");

    let delivered = daemon
        .handle_rpc(rpc_request(
            46,
            "record_receipt",
            json!({
                "message_id": "property-1",
                "status": "delivered"
            }),
        ))
        .expect("record delivered");
    assert_eq!(delivered.result.expect("result")["updated"], json!(true));
    let trace_before = daemon
        .handle_rpc(rpc_request(
            460,
            "message_delivery_trace",
            json!({
                "message_id": "property-1"
            }),
        ))
        .expect("trace before ignored update");
    let trace_before_len = trace_before.result.expect("result")["transitions"]
        .as_array()
        .expect("trace entries")
        .len();

    let ignored = daemon
        .handle_rpc(rpc_request(
            47,
            "record_receipt",
            json!({
                "message_id": "property-1",
                "status": "sent: direct"
            }),
        ))
        .expect("record after terminal");
    let ignored_result = ignored.result.expect("result");
    assert_eq!(ignored_result["updated"], json!(false));
    assert_eq!(ignored_result["status"], json!("delivered"));
    let trace_after = daemon
        .handle_rpc(rpc_request(
            470,
            "message_delivery_trace",
            json!({
                "message_id": "property-1"
            }),
        ))
        .expect("trace after ignored update");
    let trace_after_len =
        trace_after.result.expect("result")["transitions"].as_array().expect("trace entries").len();
    assert_eq!(
        trace_after_len, trace_before_len,
        "ignored terminal updates must not append delivery trace entries"
    );

    let status = daemon
        .handle_rpc(rpc_request(
            48,
            "sdk_status_v2",
            json!({
                "message_id": "property-1"
            }),
        ))
        .expect("status");
    assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("delivered"));
}

#[test]
fn record_receipt_preserves_sent_over_sending_regression() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon
        .handle_rpc(rpc_request(
            481,
            "send_message_v2",
            json!({
                "id": "property-sent-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");

    let sent = daemon
        .handle_rpc(rpc_request(
            482,
            "record_receipt",
            json!({
                "message_id": "property-sent-1",
                "status": "sent: propagated resource"
            }),
        ))
        .expect("record sent");
    assert_eq!(sent.result.expect("sent result")["updated"], json!(true));

    let ignored = daemon
        .handle_rpc(rpc_request(
            483,
            "record_receipt",
            json!({
                "message_id": "property-sent-1",
                "status": "sending: propagated resource"
            }),
        ))
        .expect("record sending after sent");
    let ignored_result = ignored.result.expect("ignored result");
    assert_eq!(ignored_result["updated"], json!(false));
    assert_eq!(ignored_result["status"], json!("sent: propagated resource"));

    let status = daemon
        .handle_rpc(rpc_request(
            484,
            "sdk_status_v2",
            json!({
                "message_id": "property-sent-1"
            }),
        ))
        .expect("status");
    assert_eq!(
        status.result.expect("status result")["message"]["receipt_status"],
        json!("sent: propagated resource")
    );
}

#[test]
fn record_receipt_preserves_detailed_failed_status() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "receipt-failed-before-update".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: None,
            receipt_status: Some("failed: no path".to_string()),
        })
        .expect("store failed message");

    let receipt = daemon
        .handle_rpc(rpc_request(
            49,
            "record_receipt",
            json!({
                "message_id": "receipt-failed-before-update",
                "status": "delivered"
            }),
        ))
        .expect("record receipt after detailed failure");
    let result = receipt.result.expect("receipt result");
    assert_eq!(result["updated"], json!(false));
    assert_eq!(result["status"], json!("failed: no path"));

    let status = daemon
        .handle_rpc(rpc_request(
            50,
            "sdk_status_v2",
            json!({
                "message_id": "receipt-failed-before-update"
            }),
        ))
        .expect("status");
    assert_eq!(
        status.result.expect("status result")["message"]["receipt_status"],
        json!("failed: no path")
    );
}

#[test]
fn sdk_property_event_sequence_is_monotonic() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .emit_event(RpcEvent { event_type: "property".to_string(), payload: json!({ "idx": 1 }) });
    daemon
        .emit_event(RpcEvent { event_type: "property".to_string(), payload: json!({ "idx": 2 }) });

    let response = daemon
        .handle_rpc(rpc_request(
            49,
            "sdk_poll_events_v2",
            json!({
                "cursor": null,
                "max": 2
            }),
        ))
        .expect("poll");
    let events =
        response.result.expect("result")["events"].as_array().expect("events array").to_vec();
    assert_eq!(events.len(), 2);
    let first = events[0]["seq_no"].as_u64().expect("first seq");
    let second = events[1]["seq_no"].as_u64().expect("second seq");
    assert!(second > first, "event sequence must be strictly increasing");
}

#[test]
fn sdk_property_cursor_churn_keeps_monotonic_progress() {
    let daemon = RpcDaemon::test_instance();
    for idx in 0..96_u64 {
        daemon.emit_event(RpcEvent {
            event_type: "property_churn".to_string(),
            payload: json!({ "idx": idx }),
        });
    }

    let mut cursor: Option<String> = None;
    let mut last_seq = 0_u64;
    let mut seen = HashSet::new();

    for iteration in 0..256_u64 {
        let response = daemon
            .handle_rpc(rpc_request(
                5_000 + iteration,
                "sdk_poll_events_v2",
                json!({
                    "cursor": cursor.clone(),
                    "max": ((iteration % 7) + 1) as usize,
                }),
            ))
            .expect("poll");
        assert!(response.error.is_none(), "poll should remain stable under churn");
        let result = response.result.expect("result");
        let events = result["events"].as_array().expect("events array");
        for event in events {
            let seq = event["seq_no"].as_u64().expect("sequence number");
            assert!(seq > last_seq, "sequence must be strictly increasing");
            assert!(seen.insert(seq), "sequence IDs must not repeat");
            last_seq = seq;
        }

        cursor = result["next_cursor"].as_str().map(ToOwned::to_owned);
        if seen.len() >= 96 {
            break;
        }
    }

    assert_eq!(
        seen.len(),
        96,
        "variable-batch polling should consume each emitted event exactly once"
    );
}

#[test]
fn sdk_configure_v2_applies_revision_cas() {
    let daemon = RpcDaemon::test_instance();
    let first = daemon
        .handle_rpc(rpc_request(
            42,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_poll_events": 64 } }
            }),
        ))
        .expect("configure");
    assert_eq!(first.result.expect("result")["revision"], json!(1));

    let conflict = daemon
        .handle_rpc(rpc_request(
            43,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_poll_events": 32 } }
            }),
        ))
        .expect("configure conflict");
    assert_eq!(conflict.error.expect("error").code, "SDK_CONFIG_CONFLICT");
}

#[test]
fn sdk_configure_v2_validates_patch_before_commit_and_revision_bump() {
    let daemon = RpcDaemon::test_instance();
    let invalid = daemon
        .handle_rpc(rpc_request(
            430,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "overflow_policy": "block" }
            }),
        ))
        .expect("configure invalid patch");
    assert_eq!(
        invalid.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "invalid patch should fail before config commit"
    );

    let valid = daemon
        .handle_rpc(rpc_request(
            431,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_poll_events": 64 } }
            }),
        ))
        .expect("configure valid patch");
    assert_eq!(
        valid.result.expect("result")["revision"],
        json!(1),
        "failed patch must not consume config revision"
    );
}
