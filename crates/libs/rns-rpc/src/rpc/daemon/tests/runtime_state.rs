use std::sync::{
    atomic::{AtomicBool as StdAtomicBool, Ordering as StdOrdering},
    mpsc as std_mpsc, Mutex as StdMutex,
};
use std::time::{Duration as StdDuration, Instant as StdInstant};

struct PendingOutboundBridge;

impl OutboundBridge for PendingOutboundBridge {
    fn deliver(
        &self,
        _record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }
}

struct SlowOutboundBridge {
    started_tx: StdMutex<Option<std_mpsc::Sender<()>>>,
    release_rx: StdMutex<std_mpsc::Receiver<()>>,
    blocked_once: StdAtomicBool,
}

impl SlowOutboundBridge {
    fn new(started_tx: std_mpsc::Sender<()>, release_rx: std_mpsc::Receiver<()>) -> Self {
        Self {
            started_tx: StdMutex::new(Some(started_tx)),
            release_rx: StdMutex::new(release_rx),
            blocked_once: StdAtomicBool::new(false),
        }
    }
}

impl OutboundBridge for SlowOutboundBridge {
    fn deliver(
        &self,
        _record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        if !self.blocked_once.swap(true, StdOrdering::SeqCst) {
            if let Some(started_tx) = self.started_tx.lock().expect("started mutex").take() {
                let _ = started_tx.send(());
            }
            let _ = self
                .release_rx
                .lock()
                .expect("release mutex")
                .recv_timeout(StdDuration::from_secs(1));
        }
        Ok(())
    }
}

#[test]
fn sdk_cancel_message_v2_distinguishes_not_found_and_too_late() {
    let daemon = RpcDaemon::test_instance();

    let not_found = daemon
        .handle_rpc(rpc_request(6, "sdk_cancel_message_v2", json!({ "message_id": "missing" })))
        .expect("cancel missing");
    assert_eq!(not_found.result.expect("result")["result"], json!("NotFound"));

    let send = daemon
        .handle_rpc(rpc_request(
            7,
            "send_message_v2",
            json!({
                "id": "outbound-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");
    assert!(send.error.is_none());

    let too_late = daemon
        .handle_rpc(rpc_request(8, "sdk_cancel_message_v2", json!({ "message_id": "outbound-1" })))
        .expect("cancel");
    assert_eq!(too_late.result.expect("result")["result"], json!("TooLateToCancel"));
}

#[test]
fn sdk_cancel_message_v2_preserves_detailed_failed_status() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "failed-before-cancel".to_string(),
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

    let cancel = daemon
        .handle_rpc(rpc_request(
            9,
            "sdk_cancel_message_v2",
            json!({ "message_id": "failed-before-cancel" }),
        ))
        .expect("cancel failed message");
    assert_eq!(cancel.result.expect("cancel result")["result"], json!("AlreadyTerminal"));

    let status = daemon
        .handle_rpc(rpc_request(
            10,
            "sdk_status_v2",
            json!({ "message_id": "failed-before-cancel" }),
        ))
        .expect("status");
    assert_eq!(
        status.result.expect("status result")["message"]["receipt_status"],
        json!("failed: no path")
    );
}

#[test]
fn send_with_bridge_stays_in_sending_until_acknowledged() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "bridge-node".to_string(),
        Some(Arc::new(PendingOutboundBridge)),
        None,
    );

    let send = daemon
        .handle_rpc(rpc_request(
            9,
            "send_message_v2",
            json!({
                "id": "pending-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");
    assert!(send.error.is_none());

    let status = daemon
        .handle_rpc(rpc_request(10, "sdk_status_v2", json!({ "message_id": "pending-1" })))
        .expect("status");
    assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("sending"));

    let trace = daemon
        .handle_rpc(rpc_request(11, "message_delivery_trace", json!({ "message_id": "pending-1" })))
        .expect("trace");
    let trace_result = trace.result.expect("result");
    let transitions = trace_result["transitions"].as_array().expect("transitions");
    assert!(
        transitions.iter().any(|entry| entry["status"] == json!("sending")),
        "bridge-backed sends should expose a non-terminal sending transition"
    );
    assert!(
        transitions.iter().all(|entry| {
            entry["status"].as_str().map_or(true, |status| !status.starts_with("sent:"))
        }),
        "bridge-backed sends must not be marked sent before transport acknowledgements arrive"
    );
}

#[test]
fn bridge_backed_send_schedules_without_waiting_on_slow_delivery() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let (started_tx, started_rx) = std_mpsc::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "slow-bridge-node".to_string(),
        Some(Arc::new(SlowOutboundBridge::new(started_tx, release_rx))),
        None,
    );

    let first = daemon
        .handle_rpc(rpc_request(
            12,
            "send_message_v2",
            json!({
                "id": "slow-bridge-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("first send");
    assert!(first.error.is_none());
    started_rx
        .recv_timeout(StdDuration::from_secs(1))
        .expect("delivery worker should start first delivery");

    let started = StdInstant::now();
    let second = daemon
        .handle_rpc(rpc_request(
            13,
            "send_message_v2",
            json!({
                "id": "slow-bridge-2",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("second send");
    assert!(second.error.is_none());
    assert!(
        started.elapsed() < StdDuration::from_millis(20),
        "send_message_v2 should enqueue delivery work instead of waiting on a slow bridge"
    );

    release_tx.send(()).expect("release slow bridge");
}

#[test]
fn outbound_lxm_queries_report_progress_and_stamp_costs() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "outbound-query-node".to_string(),
        Some(Arc::new(PendingOutboundBridge)),
        None,
    );

    let send = daemon
        .handle_rpc(rpc_request(
            12,
            "sdk_send_v2",
            json!({
                "id": "outbound-query-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello",
                "stamp_cost": 7,
                "fields": {
                    "_lxmf": {
                        "lxm_hash": "lxm-query-hash",
                        "propagation_target_cost": 9
                    }
                }
            }),
        ))
        .expect("send");
    assert!(send.error.is_none());

    let progress = daemon
        .handle_rpc(rpc_request(
            13,
            "get_outbound_progress",
            json!({ "lxm_hash": "lxm-query-hash" }),
        ))
        .expect("progress")
        .result
        .expect("progress result");
    assert_eq!(progress["message_id"], json!("outbound-query-1"));
    assert_eq!(progress["progress"].as_f64(), Some(0.01));

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            14,
            "get_outbound_lxm_stamp_cost",
            json!({ "lxm_hash": "lxm-query-hash" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"].as_u64(), Some(7));

    let propagation_stamp_cost = daemon
        .handle_rpc(rpc_request(
            15,
            "get_outbound_lxm_propagation_stamp_cost",
            json!({ "message_id": "outbound-query-1" }),
        ))
        .expect("propagation stamp cost")
        .result
        .expect("propagation stamp cost result");
    assert_eq!(propagation_stamp_cost["propagation_stamp_cost"].as_u64(), Some(9));

    daemon
        .handle_rpc(rpc_request(
            16,
            "record_receipt",
            json!({
                "message_id": "outbound-query-1",
                "status": "delivered"
            }),
        ))
        .expect("record delivered");
    let delivered_progress = daemon
        .handle_rpc(rpc_request(
            17,
            "get_outbound_progress",
            json!({ "message_id": "outbound-query-1" }),
        ))
        .expect("delivered progress")
        .result
        .expect("delivered progress result");
    assert_eq!(delivered_progress["progress"].as_f64(), Some(1.0));
}

#[test]
fn outbound_lxm_progress_does_not_mark_failed_or_cancelled_as_complete() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, status) in [
        ("failed-outbound-query", "failed: no path"),
        ("cancelled-outbound-query", "cancelled"),
        ("rejected-outbound-query", "rejected"),
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
                fields: Some(json!({
                    "_lxmf": {
                        "lxm_hash": format!("{message_id}-hash")
                    }
                })),
                receipt_status: Some(status.to_string()),
            })
            .expect("store outbound");

        let progress = daemon
            .handle_rpc(rpc_request(
                18,
                "get_outbound_progress",
                json!({ "message_id": message_id }),
            ))
            .expect("progress")
            .result
            .expect("progress result");
        assert_eq!(progress["message_id"], json!(message_id));
        assert_eq!(progress["progress"], JsonValue::Null);
    }
}

#[test]
fn outbound_lxm_progress_does_not_treat_completed_stamp_work_as_delivery_complete() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "stamp-ready-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_state": "ready",
                    "stamp_target_cost": 7,
                    "propagation_stamp_state": "ready",
                    "propagation_stamp_target_cost": 9
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let progress = daemon
        .handle_rpc(rpc_request(
            19,
            "get_outbound_progress",
            json!({ "message_id": "stamp-ready-outbound-query" }),
        ))
        .expect("progress")
        .result
        .expect("progress result");
    assert_eq!(progress["message_id"], json!("stamp-ready-outbound-query"));
    assert_eq!(progress["progress"].as_f64(), Some(0.01));
}

#[test]
fn outbound_lxm_progress_normalizes_stamp_state_strings() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, lxmf) in [
        ("cancelled-stamp-state-outbound-query", json!({ "stamp_state": " CANCELLED " })),
        (
            "failed-propagation-stamp-state-outbound-query",
            json!({ "propagation_stamp_state": " FAILED " }),
        ),
        ("generating-stamp-state-outbound-query", json!({ "stamp_state": " GENERATING " })),
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

        let progress = daemon
            .handle_rpc(rpc_request(
                20,
                "get_outbound_progress",
                json!({ "message_id": message_id }),
            ))
            .expect("progress")
            .result
            .expect("progress result");
        if message_id == "generating-stamp-state-outbound-query" {
            assert_eq!(progress["progress"].as_f64(), Some(0.0));
        } else {
            assert_eq!(progress["progress"], JsonValue::Null);
        }
    }
}

#[test]
fn outbound_lxm_stamp_cost_is_null_when_ticket_stamp_is_used() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "ticket-stamped-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_kind": "ticket",
                    "stamp_target_cost": 256
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            18,
            "get_outbound_lxm_stamp_cost",
            json!({ "message_id": "ticket-stamped-outbound-query" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"], JsonValue::Null);
}

#[test]
fn outbound_lxm_stamp_cost_ignores_null_or_empty_ticket_markers() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, lxmf) in [
        (
            "null-ticket-outbound-query",
            json!({
                "stamp_target_cost": 7,
                "outbound_ticket": null,
                "stamp_ticket_source": null
            }),
        ),
        (
            "empty-ticket-outbound-query",
            json!({
                "stamp_target_cost": 8,
                "outbound_ticket": "",
                "stamp_ticket_source": "   "
            }),
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
            .handle_rpc(rpc_request(
                20,
                "get_outbound_lxm_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert!(stamp_cost["stamp_cost"].as_u64().is_some());
    }
}

#[test]
fn outbound_lxm_stamp_cost_queries_accept_string_cost_metadata() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "string-cost-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_target_cost": " 7.0 ",
                    "propagation_stamp_target_cost": "9.0"
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            21,
            "get_outbound_lxm_stamp_cost",
            json!({ "message_id": "string-cost-outbound-query" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"].as_u64(), Some(7));

    let propagation_stamp_cost = daemon
        .handle_rpc(rpc_request(
            22,
            "get_outbound_lxm_propagation_stamp_cost",
            json!({ "message_id": "string-cost-outbound-query" }),
        ))
        .expect("propagation stamp cost")
        .result
        .expect("propagation stamp cost result");
    assert_eq!(propagation_stamp_cost["propagation_stamp_cost"].as_u64(), Some(9));
}

#[test]
fn outbound_lxm_stamp_cost_queries_accept_whole_float_cost_metadata() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .accept_inbound(MessageRecord {
            id: "float-cost-outbound-query".to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "content".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({
                "_lxmf": {
                    "stamp_target_cost": 7.0,
                    "propagation_stamp_target_cost": 9.0
                }
            })),
            receipt_status: Some("sending".to_string()),
        })
        .expect("store outbound");

    let stamp_cost = daemon
        .handle_rpc(rpc_request(
            23,
            "get_outbound_lxm_stamp_cost",
            json!({ "message_id": "float-cost-outbound-query" }),
        ))
        .expect("stamp cost")
        .result
        .expect("stamp cost result");
    assert_eq!(stamp_cost["stamp_cost"].as_u64(), Some(7));

    let propagation_stamp_cost = daemon
        .handle_rpc(rpc_request(
            24,
            "get_outbound_lxm_propagation_stamp_cost",
            json!({ "message_id": "float-cost-outbound-query" }),
        ))
        .expect("propagation stamp cost")
        .result
        .expect("propagation stamp cost result");
    assert_eq!(propagation_stamp_cost["propagation_stamp_cost"].as_u64(), Some(9));
}

#[test]
fn outbound_lxm_stamp_cost_queries_reject_fractional_cost_metadata() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, stamp_cost_value, propagation_stamp_cost_value) in [
        ("fractional-cost-outbound-query", json!(7.5), json!(9.25)),
        ("negative-float-cost-outbound-query", json!(-7.0), json!(-9.0)),
        ("fractional-string-cost-outbound-query", json!("7.5"), json!("9.25")),
        ("negative-string-cost-outbound-query", json!("-7.0"), json!("-9.0")),
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
                fields: Some(json!({
                    "_lxmf": {
                        "stamp_target_cost": stamp_cost_value,
                        "propagation_stamp_target_cost": propagation_stamp_cost_value
                    }
                })),
                receipt_status: Some("sending".to_string()),
            })
            .expect("store outbound");

        let stamp_cost = daemon
            .handle_rpc(rpc_request(
                25,
                "get_outbound_lxm_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert_eq!(stamp_cost["stamp_cost"], JsonValue::Null);

        let propagation_stamp_cost = daemon
            .handle_rpc(rpc_request(
                26,
                "get_outbound_lxm_propagation_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("propagation stamp cost")
            .result
            .expect("propagation stamp cost result");
        assert_eq!(propagation_stamp_cost["propagation_stamp_cost"], JsonValue::Null);
    }
}

#[test]
fn outbound_lxm_stamp_cost_queries_are_null_after_terminal_status() {
    let daemon = RpcDaemon::test_instance();
    for (message_id, status) in [
        ("delivered-cost-query", "delivered"),
        ("sent-cost-query", "sent: direct"),
        ("failed-cost-query", "failed: no path"),
        ("cancelled-cost-query", "cancelled"),
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
                fields: Some(json!({
                    "_lxmf": {
                        "stamp_target_cost": 7,
                        "propagation_stamp_target_cost": 9
                    }
                })),
                receipt_status: Some(status.to_string()),
            })
            .expect("store outbound");

        let stamp_cost = daemon
            .handle_rpc(rpc_request(
                20,
                "get_outbound_lxm_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("stamp cost")
            .result
            .expect("stamp cost result");
        assert_eq!(stamp_cost["stamp_cost"], JsonValue::Null);

        let propagation_stamp_cost = daemon
            .handle_rpc(rpc_request(
                21,
                "get_outbound_lxm_propagation_stamp_cost",
                json!({ "message_id": message_id }),
            ))
            .expect("propagation stamp cost")
            .result
            .expect("propagation stamp cost result");
        assert_eq!(propagation_stamp_cost["propagation_stamp_cost"], JsonValue::Null);
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

#[test]
fn sdk_configure_v2_rejects_out_of_bounds_event_stream_limits() {
    let daemon = RpcDaemon::test_instance();
    let below_min_batch = daemon
        .handle_rpc(rpc_request(
            434,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_batch_bytes": 512 } }
            }),
        ))
        .expect("configure");
    assert_eq!(below_min_batch.error.expect("error").code, "SDK_VALIDATION_INVALID_ARGUMENT");

    let extension_limit_overflow = daemon
        .handle_rpc(rpc_request(
            435,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_extension_keys": 64 } }
            }),
        ))
        .expect("configure");
    assert_eq!(
        extension_limit_overflow.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT"
    );

    let unknown_event_stream_key = daemon
        .handle_rpc(rpc_request(
            4351,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "unknown_limit": 10 } }
            }),
        ))
        .expect("configure");
    assert_eq!(unknown_event_stream_key.error.expect("error").code, "SDK_CONFIG_UNKNOWN_KEY");

    let inconsistent_event_and_batch = daemon
        .handle_rpc(rpc_request(
            436,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_stream": {
                        "max_event_bytes": 4096,
                        "max_batch_bytes": 2048
                    }
                }
            }),
        ))
        .expect("configure");
    assert_eq!(
        inconsistent_event_and_batch.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT"
    );
}

#[test]
fn sdk_configure_v2_validates_and_applies_store_forward_policy_patch() {
    let daemon = RpcDaemon::test_instance();

    let invalid = daemon
        .handle_rpc(rpc_request(
            4361,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 0
                    }
                }
            }),
        ))
        .expect("configure invalid");
    assert_eq!(
        invalid.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "store_forward max_messages=0 should fail validation"
    );

    let valid = daemon
        .handle_rpc(rpc_request(
            4362,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 1024,
                        "max_message_age_ms": 120000,
                        "capacity_policy": "drop_oldest",
                        "eviction_priority": "terminal_first"
                    }
                }
            }),
        ))
        .expect("configure valid");
    assert!(valid.error.is_none());
    assert_eq!(valid.result.expect("result")["revision"], json!(1));

    let runtime_config =
        daemon.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
    assert_eq!(runtime_config["store_forward"]["max_messages"], json!(1024));
    assert_eq!(runtime_config["store_forward"]["capacity_policy"], json!("drop_oldest"));
}

#[test]
fn sdk_configure_v2_validates_and_applies_event_sink_patch() {
    let daemon = RpcDaemon::test_instance();

    let invalid = daemon
        .handle_rpc(rpc_request(
            4363,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "allow_kinds": []
                    }
                }
            }),
        ))
        .expect("configure invalid");
    assert_eq!(
        invalid.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "event_sink allow_kinds=[] should fail validation"
    );

    let valid = daemon
        .handle_rpc(rpc_request(
            4364,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "max_event_bytes": 32768,
                        "allow_kinds": ["webhook", "mqtt"]
                    }
                }
            }),
        ))
        .expect("configure valid");
    assert!(valid.error.is_none());
    assert_eq!(valid.result.expect("result")["revision"], json!(1));

    let runtime_config =
        daemon.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
    assert_eq!(runtime_config["event_sink"]["enabled"], json!(true));
    assert_eq!(runtime_config["event_sink"]["allow_kinds"], json!(["webhook", "mqtt"]));
}

#[test]
fn sdk_dispatch_maps_unknown_fields_to_validation_unknown_field() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(rpc_request(
            432,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": { "profile": "desktop-full" },
                "unexpected_field": true
            }),
        ))
        .expect("negotiate");
    assert_eq!(
        response.error.expect("error").code,
        "SDK_VALIDATION_UNKNOWN_FIELD",
        "sdk requests with unknown fields should return typed validation errors"
    );
}

#[test]
fn sdk_dispatch_maps_missing_params_to_validation_invalid_argument() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(RpcRequest { id: 433, method: "sdk_shutdown_v2".to_string(), params: None })
        .expect("shutdown response");
    assert_eq!(
        response.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "sdk requests without params should return typed validation errors"
    );
}

#[test]
fn sdk_shutdown_v2_accepts_graceful_mode() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(rpc_request(
            44,
            "sdk_shutdown_v2",
            json!({
                "mode": "graceful"
            }),
        ))
        .expect("shutdown");
    assert!(response.error.is_none());
    assert_eq!(response.result.expect("result")["accepted"], json!(true));
}

#[test]
fn sdk_snapshot_v2_returns_runtime_summary() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon.handle_rpc(rpc_request(
        9,
        "sdk_negotiate_v2",
        json!({
            "supported_contract_versions": [2],
            "requested_capabilities": [],
            "config": { "profile": "desktop-full" }
        }),
    ));

    let snapshot = daemon
        .handle_rpc(rpc_request(10, "sdk_snapshot_v2", json!({ "include_counts": true })))
        .expect("snapshot");
    assert!(snapshot.error.is_none());
    let result = snapshot.result.expect("result");
    assert_eq!(result["runtime_id"], json!("test-identity"));
    assert_eq!(result["state"], json!("running"));
    assert!(result.get("event_stream_position").is_some());
}

#[test]
fn sdk_race_cancel_and_receipt_updates_converge_to_terminal_state() {
    let daemon = RpcDaemon::test_instance();

    for idx in 0..96_u64 {
        let message_id = format!("race-message-{idx}");
        let receive = daemon
            .handle_rpc(rpc_request(
                50_000 + (idx * 10),
                "receive_message",
                json!({
                    "id": message_id,
                    "source": "race.source",
                    "destination": "race.destination",
                    "title": "",
                    "content": "race payload",
                    "fields": null
                }),
            ))
            .expect("receive");
        assert!(receive.error.is_none(), "receive_message should succeed for race setup");

        let call_cancel = |id: u64| {
            daemon
                .handle_rpc(rpc_request(
                    id,
                    "sdk_cancel_message_v2",
                    json!({ "message_id": message_id }),
                ))
                .expect("cancel")
        };
        let call_receipt = |id: u64| {
            daemon
                .handle_rpc(rpc_request(
                    id,
                    "record_receipt",
                    json!({
                        "message_id": message_id,
                        "status": "delivered"
                    }),
                ))
                .expect("record_receipt")
        };

        let cancel = if idx % 2 == 0 {
            let cancel = call_cancel(50_000 + (idx * 10) + 1);
            let receipt = call_receipt(50_000 + (idx * 10) + 2);
            assert!(receipt.error.is_none(), "record_receipt race call should stay error-free");
            cancel
        } else {
            let receipt = call_receipt(50_000 + (idx * 10) + 2);
            assert!(receipt.error.is_none(), "record_receipt race call should stay error-free");
            call_cancel(50_000 + (idx * 10) + 1)
        };

        let cancel_payload = cancel.result.expect("cancel result");
        let cancel_result = cancel_payload["result"].as_str().expect("cancel result string");
        assert!(
            matches!(cancel_result, "Accepted" | "AlreadyTerminal"),
            "cancel race should resolve to accepted or already-terminal"
        );

        let status = daemon
            .handle_rpc(rpc_request(
                50_000 + (idx * 10) + 3,
                "sdk_status_v2",
                json!({ "message_id": message_id }),
            ))
            .expect("status");
        let status_payload = status.result.expect("status result");
        let receipt_status =
            status_payload["message"]["receipt_status"].as_str().expect("status receipt_status");
        assert!(
            matches!(receipt_status, "cancelled" | "delivered"),
            "race must converge to a single terminal status"
        );

        let second_cancel = daemon
            .handle_rpc(rpc_request(
                50_000 + (idx * 10) + 4,
                "sdk_cancel_message_v2",
                json!({ "message_id": message_id }),
            ))
            .expect("second cancel");
        assert_eq!(
            second_cancel.result.expect("second cancel result")["result"],
            json!("AlreadyTerminal")
        );
    }
}
