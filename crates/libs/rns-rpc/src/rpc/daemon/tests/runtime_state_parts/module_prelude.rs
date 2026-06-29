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

struct PipelineStatusBridge;

impl OutboundBridge for PipelineStatusBridge {
    fn deliver(
        &self,
        _record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn delivery_pipeline_status(&self) -> Option<serde_json::Value> {
        Some(json!({
            "queued_total": 3,
            "in_flight_total": 1,
            "rejected_queue_full_total": 0,
        }))
    }
}

struct SlowOutboundBridge {
    started_tx: StdMutex<Option<std_mpsc::Sender<()>>>,
    release_rx: StdMutex<std_mpsc::Receiver<()>>,
    blocked_once: StdAtomicBool,
}

struct BlockingOutboundBridge {
    started_tx: StdMutex<std_mpsc::Sender<String>>,
    release_rx: StdMutex<std_mpsc::Receiver<()>>,
}

impl BlockingOutboundBridge {
    fn new(started_tx: std_mpsc::Sender<String>, release_rx: std_mpsc::Receiver<()>) -> Self {
        Self { started_tx: StdMutex::new(started_tx), release_rx: StdMutex::new(release_rx) }
    }
}

impl OutboundBridge for BlockingOutboundBridge {
    fn deliver(
        &self,
        record: &MessageRecord,
        _options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let _ = self.started_tx.lock().expect("started mutex").send(record.id.clone());
        let _ =
            self.release_rx.lock().expect("release mutex").recv_timeout(StdDuration::from_secs(2));
        Ok(())
    }
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
fn sdk_cancel_message_v2_exposes_negative_results_in_lifecycle_trace() {
    let daemon = RpcDaemon::test_instance();

    let not_found = daemon
        .handle_rpc(rpc_request(
            60,
            "sdk_cancel_message_v2",
            json!({ "message_id": "missing-observable" }),
        ))
        .expect("cancel missing");
    assert_eq!(not_found.result.expect("not found result")["result"], json!("NotFound"));

    let send = daemon
        .handle_rpc(rpc_request(
            61,
            "send_message_v2",
            json!({
                "id": "sent-observable",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "hello"
            }),
        ))
        .expect("send");
    assert!(send.error.is_none());

    let too_late = daemon
        .handle_rpc(rpc_request(
            62,
            "sdk_cancel_message_v2",
            json!({ "message_id": "sent-observable" }),
        ))
        .expect("cancel sent");
    assert_eq!(too_late.result.expect("too late result")["result"], json!("TooLateToCancel"));

    let mut cancel_results = Vec::new();
    while let Some(event) = daemon.take_event() {
        if event.event_type != "sdk_lifecycle_trace" {
            continue;
        }
        if event.payload["method"] != json!("sdk_cancel_message_v2")
            || event.payload["phase"] != json!("finish")
            || event.payload["outcome"] != json!("ok")
        {
            continue;
        }
        if let Some(cancel_result) =
            event.payload["details"]["cancel_result"].as_str().map(ToOwned::to_owned)
        {
            cancel_results.push(cancel_result);
        }
    }

    assert!(
        cancel_results.iter().any(|result| result == "NotFound"),
        "missing-message cancellation should be observable as NotFound"
    );
    assert!(
        cancel_results.iter().any(|result| result == "TooLateToCancel"),
        "sent-message cancellation should be observable as TooLateToCancel"
    );
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

    let mut saw_already_terminal = false;
    while let Some(event) = daemon.take_event() {
        if event.event_type == "sdk_lifecycle_trace"
            && event.payload["method"] == json!("sdk_cancel_message_v2")
            && event.payload["phase"] == json!("finish")
            && event.payload["details"]["cancel_result"] == json!("AlreadyTerminal")
        {
            saw_already_terminal = true;
            break;
        }
    }
    assert!(
        saw_already_terminal,
        "terminal cancellation should be visible without status rewrite"
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
            entry["status"].as_str().is_none_or(|status| !status.starts_with("sent:"))
        }),
        "bridge-backed sends must not be marked sent before transport acknowledgements arrive"
    );
}

#[test]
fn sdk_status_v2_includes_delivery_pipeline_status_when_bridge_reports_it() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "pipeline-status-node".to_string(),
        Some(Arc::new(PipelineStatusBridge)),
        None,
    );

    let status = daemon
        .handle_rpc(rpc_request(
            12,
            "sdk_status_v2",
            json!({ "message_id": "pipeline-status-missing" }),
        ))
        .expect("status");
    let result = status.result.expect("result");

    assert_eq!(result["delivery_pipeline"]["queued_total"], json!(3));
    assert_eq!(result["delivery_pipeline"]["in_flight_total"], json!(1));
    assert_eq!(result["delivery_pipeline"]["rejected_queue_full_total"], json!(0));
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
fn app_delivery_cancel_accepts_queued_message_before_bridge_handoff() {
    const BLOCKED_DELIVERIES: usize = 16;

    let store = MessagesStore::in_memory().expect("in-memory store");
    let (started_tx, started_rx) = std_mpsc::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "queued-cancel-node".to_string(),
        Some(Arc::new(BlockingOutboundBridge::new(started_tx, release_rx))),
        None,
    );

    for index in 0..BLOCKED_DELIVERIES {
        let response = daemon
            .handle_rpc(rpc_request(
                30 + index as u64,
                "send_message_v2",
                json!({
                    "id": format!("queued-cancel-blocker-{index}"),
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send blocking message");
        assert!(response.error.is_none());
    }

    let mut blocked_ids = Vec::with_capacity(BLOCKED_DELIVERIES);
    for _ in 0..BLOCKED_DELIVERIES {
        blocked_ids.push(
            started_rx
                .recv_timeout(StdDuration::from_secs(1))
                .expect("worker lane should start blocker"),
        );
    }
    assert!(
        blocked_ids.iter().all(|id| id.starts_with("queued-cancel-blocker-")),
        "all worker lanes should be occupied by blockers: {blocked_ids:?}"
    );

    let queued = daemon
        .handle_rpc(rpc_request(
            60,
            "send_message_v2",
            json!({
                "id": "queued-cancel-target",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "cancel before handoff"
            }),
        ))
        .expect("send queued target");
    assert!(queued.error.is_none());

    let cancel = daemon
        .handle_rpc(rpc_request(
            61,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.delivery.cancel",
                "kind": "command",
                "correlation_id": "queued-cancel-corr",
                "payload": {
                    "message_id": "queued-cancel-target"
                }
            }),
        ))
        .expect("cancel queued target through app envelope");
    assert!(cancel.error.is_none());
    let cancel_result = cancel.result.expect("cancel result");
    assert_eq!(cancel_result["response"]["operation_id"], json!("app.delivery.cancel"));
    assert_eq!(cancel_result["response"]["payload"]["message_id"], json!("queued-cancel-target"));
    assert_eq!(cancel_result["response"]["payload"]["result"], json!("Accepted"));

    let status = daemon
        .handle_rpc(rpc_request(
            62,
            "sdk_status_v2",
            json!({ "message_id": "queued-cancel-target" }),
        ))
        .expect("cancelled status");
    assert_eq!(
        status.result.expect("status result")["message"]["receipt_status"],
        json!("cancelled")
    );

    let trace = daemon
        .handle_rpc(rpc_request(
            63,
            "message_delivery_trace",
            json!({ "message_id": "queued-cancel-target" }),
        ))
        .expect("delivery trace");
    let trace_result = trace.result.expect("trace result");
    let transitions = trace_result["transitions"].as_array().expect("transitions");
    assert!(
        transitions.iter().any(|entry| entry["status"] == json!("cancelled")),
        "queued cancellation should be visible in the delivery trace"
    );

    let mut saw_delivery_cancelled = false;
    let mut saw_envelope_lifecycle = false;
    while let Some(event) = daemon.take_event() {
        if event.event_type == "delivery_cancelled"
            && event.payload["message_id"] == json!("queued-cancel-target")
            && event.payload["result"] == json!("Accepted")
        {
            saw_delivery_cancelled = true;
        }
        if event.event_type == "sdk_lifecycle_trace"
            && event.payload["method"] == json!("sdk_envelope_execute_v2")
            && event.payload["phase"] == json!("finish")
            && event.payload["outcome"] == json!("ok")
            && event.payload["details"]["operation_id"] == json!("app.delivery.cancel")
            && event.payload["details"]["message_id"] == json!("queued-cancel-target")
            && event.payload["details"]["cancel_result"] == json!("Accepted")
        {
            saw_envelope_lifecycle = true;
        }
    }
    assert!(saw_delivery_cancelled, "accepted cancel should emit delivery_cancelled");
    assert!(
        saw_envelope_lifecycle,
        "envelope execution should expose cancel metadata in lifecycle trace"
    );

    for _ in 0..BLOCKED_DELIVERIES {
        release_tx.send(()).expect("release blocker");
    }
    assert!(
        started_rx.recv_timeout(StdDuration::from_millis(250)).is_err(),
        "cancelled queued message must not be handed off to the outbound bridge"
    );
}

#[test]
fn outbound_delivery_worker_uses_bounded_parallel_lanes() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let (started_tx, started_rx) = std_mpsc::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "parallel-bridge-node".to_string(),
        Some(Arc::new(BlockingOutboundBridge::new(started_tx, release_rx))),
        None,
    );

    for index in 0..2 {
        let response = daemon
            .handle_rpc(rpc_request(
                20 + index,
                "send_message_v2",
                json!({
                    "id": format!("parallel-bridge-{index}"),
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");
        assert!(response.error.is_none());
    }

    let first =
        started_rx.recv_timeout(StdDuration::from_secs(1)).expect("first delivery should start");
    let second = started_rx
        .recv_timeout(StdDuration::from_secs(1))
        .expect("second delivery should start before first is released");
    assert_ne!(first, second);

    release_tx.send(()).expect("release first");
    release_tx.send(()).expect("release second");
}

#[test]
fn app_delivery_cancel_envelope_cancels_deferred_bridge_handoff_and_traces() {
    const BLOCKED_DELIVERIES: usize = 16;

    let store = MessagesStore::in_memory().expect("in-memory store");
    let (started_tx, started_rx) = std_mpsc::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "cancel-deferred-bridge-node".to_string(),
        Some(Arc::new(BlockingOutboundBridge::new(started_tx, release_rx))),
        None,
    );

    for index in 0..BLOCKED_DELIVERIES {
        let response = daemon
            .handle_rpc(rpc_request(
                3_000 + index as u64,
                "sdk_send_v2",
                json!({
                    "id": format!("cancel-blocker-{index}"),
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hold worker lane"
                }),
            ))
            .expect("send blocker");
        assert!(response.error.is_none());
    }

    for _ in 0..BLOCKED_DELIVERIES {
        let started = started_rx
            .recv_timeout(StdDuration::from_secs(1))
            .expect("each delivery worker lane should be blocked");
        assert!(started.starts_with("cancel-blocker-"));
    }

    let send = daemon
        .handle_rpc(rpc_request(
            3_100,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.delivery.send",
                "kind": "command",
                "correlation_id": "deferred-send-corr",
                "payload": {
                    "id": "cancel-deferred-target",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "cancel before handoff"
                }
            }),
        ))
        .expect("send target envelope");
    assert!(send.error.is_none());
    let send_result = send.result.expect("send result");
    assert_eq!(send_result["response"]["operation_id"], json!("app.delivery.send"));
    assert_eq!(send_result["response"]["correlation_id"], json!("deferred-send-corr"));
    assert_eq!(
        send_result["response"]["payload"]["message_id"],
        json!("cancel-deferred-target")
    );

    let cancel = daemon
        .handle_rpc(rpc_request(
            3_101,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.delivery.cancel",
                "kind": "command",
                "correlation_id": "deferred-cancel-corr",
                "payload": {
                    "message_id": "cancel-deferred-target"
                }
            }),
        ))
        .expect("cancel target envelope");
    assert!(cancel.error.is_none());
    let cancel_result = cancel.result.expect("cancel result");
    assert_eq!(cancel_result["response"]["operation_id"], json!("app.delivery.cancel"));
    assert_eq!(cancel_result["response"]["correlation_id"], json!("deferred-cancel-corr"));
    assert_eq!(cancel_result["response"]["payload"]["result"], json!("Accepted"));

    let status = daemon
        .handle_rpc(rpc_request(
            3_102,
            "sdk_status_v2",
            json!({ "message_id": "cancel-deferred-target" }),
        ))
        .expect("status after cancel");
    assert_eq!(
        status.result.expect("status result")["message"]["receipt_status"],
        json!("cancelled")
    );

    let trace = daemon
        .handle_rpc(rpc_request(
            3_103,
            "message_delivery_trace",
            json!({ "message_id": "cancel-deferred-target" }),
        ))
        .expect("delivery trace after cancel");
    let trace_result = trace.result.expect("trace result");
    let transitions = trace_result["transitions"].as_array().expect("trace transitions");
    assert!(
        transitions.iter().any(|entry| entry["status"] == json!("cancelled")),
        "cancelled deferred sends should record a cancelled delivery transition"
    );
    assert!(
        transitions.iter().all(|entry| {
            entry["status"].as_str().is_none_or(|status| !status.starts_with("sent:"))
        }),
        "cancelled deferred sends must not be marked sent"
    );

    let mut saw_cancel_lifecycle = false;
    let mut saw_delivery_cancelled = false;
    while let Some(event) = daemon.take_event() {
        if event.event_type == "delivery_cancelled"
            && event.payload["message_id"] == json!("cancel-deferred-target")
            && event.payload["result"] == json!("Accepted")
        {
            saw_delivery_cancelled = true;
        }
        if event.event_type == "sdk_lifecycle_trace"
            && event.payload["method"] == json!("sdk_cancel_message_v2")
            && event.payload["phase"] == json!("finish")
            && event.payload["outcome"] == json!("ok")
            && event.payload["details"]["cancel_result"] == json!("Accepted")
        {
            saw_cancel_lifecycle = true;
        }
    }
    assert!(
        saw_delivery_cancelled,
        "app.delivery.cancel should publish a delivery_cancelled event"
    );
    assert!(
        saw_cancel_lifecycle,
        "app.delivery.cancel should preserve sdk_cancel_message_v2 lifecycle details"
    );

    for _ in 0..BLOCKED_DELIVERIES {
        release_tx.send(()).expect("release blocked delivery");
    }
    match started_rx.recv_timeout(StdDuration::from_secs(1)) {
        Err(std_mpsc::RecvTimeoutError::Timeout) => {}
        Ok(delivered_id) => {
            panic!("cancelled deferred message was handed to the bridge: {delivered_id}")
        }
        Err(err) => panic!("delivery worker start channel failed: {err}"),
    }
}

#[test]
fn app_delivery_cancel_is_too_late_after_worker_claims_bridge_handoff() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let (started_tx, started_rx) = std_mpsc::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "claimed-cancel-node".to_string(),
        Some(Arc::new(BlockingOutboundBridge::new(started_tx, release_rx))),
        None,
    );

    let send = daemon
        .handle_rpc(rpc_request(
            3_200,
            "sdk_send_v2",
            json!({
                "id": "claimed-cancel-target",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "worker claims before cancel"
            }),
        ))
        .expect("send target");
    assert!(send.error.is_none());
    let started = started_rx
        .recv_timeout(StdDuration::from_secs(1))
        .expect("worker should claim handoff");
    assert_eq!(started, "claimed-cancel-target");

    let cancel = daemon
        .handle_rpc(rpc_request(
            3_201,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.delivery.cancel",
                "kind": "command",
                "correlation_id": "claimed-cancel-corr",
                "payload": {
                    "message_id": "claimed-cancel-target"
                }
            }),
        ))
        .expect("cancel claimed target");
    assert!(cancel.error.is_none());
    let cancel_result = cancel.result.expect("cancel result");
    assert_eq!(cancel_result["response"]["payload"]["result"], json!("TooLateToCancel"));

    let status = daemon
        .handle_rpc(rpc_request(
            3_202,
            "sdk_status_v2",
            json!({ "message_id": "claimed-cancel-target" }),
        ))
        .expect("status after claimed cancel");
    assert_eq!(
        status.result.expect("status result")["message"]["receipt_status"],
        json!("sending")
    );

    release_tx.send(()).expect("release blocked delivery");
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
