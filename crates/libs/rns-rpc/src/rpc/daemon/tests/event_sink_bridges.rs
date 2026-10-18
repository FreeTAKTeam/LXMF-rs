use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

struct RecordingSink {
    sink_id: String,
    sink_kind: &'static str,
    fail_publish: bool,
    envelopes: Arc<Mutex<Vec<RpcEventSinkEnvelope>>>,
}

impl RecordingSink {
    fn new(
        sink_id: &str,
        sink_kind: &'static str,
        fail_publish: bool,
        envelopes: Arc<Mutex<Vec<RpcEventSinkEnvelope>>>,
    ) -> Self {
        Self {
            sink_id: sink_id.to_string(),
            sink_kind,
            fail_publish,
            envelopes,
        }
    }
}

impl EventSinkBridge for RecordingSink {
    fn sink_id(&self) -> &str {
        self.sink_id.as_str()
    }

    fn sink_kind(&self) -> &'static str {
        self.sink_kind
    }

    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> Result<(), std::io::Error> {
        if self.fail_publish {
            return Err(std::io::Error::other("forced publish failure"));
        }
        self.envelopes
            .lock()
            .expect("event sink envelopes mutex poisoned")
            .push(envelope.clone());
        Ok(())
    }
}

struct SlowFirstSink {
    started_tx: Mutex<Option<mpsc::Sender<()>>>,
    release_rx: Mutex<mpsc::Receiver<()>>,
    blocked_once: AtomicBool,
}

impl SlowFirstSink {
    fn new(started_tx: mpsc::Sender<()>, release_rx: mpsc::Receiver<()>) -> Self {
        Self {
            started_tx: Mutex::new(Some(started_tx)),
            release_rx: Mutex::new(release_rx),
            blocked_once: AtomicBool::new(false),
        }
    }
}

impl EventSinkBridge for SlowFirstSink {
    fn sink_id(&self) -> &str {
        "slow-first"
    }

    fn sink_kind(&self) -> &'static str {
        "webhook"
    }

    fn publish(&self, _envelope: &RpcEventSinkEnvelope) -> Result<(), std::io::Error> {
        if !self.blocked_once.swap(true, Ordering::SeqCst) {
            if let Some(started_tx) = self.started_tx.lock().expect("started mutex").take() {
                started_tx.send(()).expect("slow sink observer remains connected");
            }
            let _ = self
                .release_rx
                .lock()
                .expect("release mutex")
                .recv_timeout(Duration::from_secs(1));
        }
        Ok(())
    }
}

#[test]
fn sdk_event_sink_bridge_receives_redacted_payload() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let envelopes = Arc::new(Mutex::new(Vec::new()));
    let sink: Arc<dyn EventSinkBridge> = Arc::new(RecordingSink::new(
        "webhook-main",
        "webhook",
        false,
        Arc::clone(&envelopes),
    ));
    let daemon = RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        "sink-node".to_string(),
        None,
        None,
        vec![sink],
    );

    let configure = daemon
        .handle_rpc(rpc_request(
            5001,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "max_event_bytes": 32768,
                        "allow_kinds": ["webhook"]
                    },
                    "redaction": {
                        "enabled": true,
                        "sensitive_transform": "hash"
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    daemon.flush_event_sink_worker_for_test();
    envelopes.lock().expect("envelopes mutex poisoned").clear();

    daemon.emit_event(RpcEvent {
        event_type: "security_notice".to_string(),
        payload: json!({
            "token": "top-secret-token",
            "peer_id": "peer-1",
            "message": "ok"
        }),
    });
    daemon.flush_event_sink_worker_for_test();

    let captured = envelopes.lock().expect("envelopes mutex poisoned").clone();
    assert_eq!(captured.len(), 1);
    let envelope = captured.first().expect("one envelope");
    assert_eq!(envelope.runtime_id, "sink-node");
    assert_eq!(envelope.stream_id, "sdk-events");
    let token = envelope
        .event
        .payload
        .get("token")
        .and_then(JsonValue::as_str)
        .expect("token string");
    assert_ne!(token, "top-secret-token", "sensitive values must be redacted before sink dispatch");
}

#[test]
fn sdk_event_sink_bridge_respects_allow_kinds_filter() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let webhook_envelopes = Arc::new(Mutex::new(Vec::new()));
    let mqtt_envelopes = Arc::new(Mutex::new(Vec::new()));
    let webhook_sink: Arc<dyn EventSinkBridge> = Arc::new(RecordingSink::new(
        "webhook-main",
        "webhook",
        false,
        Arc::clone(&webhook_envelopes),
    ));
    let mqtt_sink: Arc<dyn EventSinkBridge> = Arc::new(RecordingSink::new(
        "mqtt-main",
        "mqtt",
        false,
        Arc::clone(&mqtt_envelopes),
    ));
    let daemon = RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        "sink-node".to_string(),
        None,
        None,
        vec![webhook_sink, mqtt_sink],
    );

    let configure = daemon
        .handle_rpc(rpc_request(
            5002,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "allow_kinds": ["mqtt"]
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    daemon.flush_event_sink_worker_for_test();
    webhook_envelopes
        .lock()
        .expect("webhook envelopes mutex")
        .clear();
    mqtt_envelopes.lock().expect("mqtt envelopes mutex").clear();

    daemon.emit_event(RpcEvent {
        event_type: "delivery_update".to_string(),
        payload: json!({ "message_id": "m-1" }),
    });
    daemon.flush_event_sink_worker_for_test();

    assert_eq!(webhook_envelopes.lock().expect("webhook envelopes mutex").len(), 0);
    assert_eq!(mqtt_envelopes.lock().expect("mqtt envelopes mutex").len(), 1);
}

#[test]
fn sdk_event_sink_bridge_failures_are_counted_in_metrics() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let sink: Arc<dyn EventSinkBridge> = Arc::new(RecordingSink::new(
        "webhook-main",
        "webhook",
        true,
        Arc::new(Mutex::new(Vec::new())),
    ));
    let daemon = RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        "sink-node".to_string(),
        None,
        None,
        vec![sink],
    );

    let configure = daemon
        .handle_rpc(rpc_request(
            5003,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "allow_kinds": ["webhook"]
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    daemon.flush_event_sink_worker_for_test();
    let baseline_errors = daemon
        .metrics_snapshot()
        .get("counters")
        .and_then(|value| value.get("sdk_event_sink_error_total"))
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);

    daemon.emit_event(RpcEvent {
        event_type: "delivery_update".to_string(),
        payload: json!({ "message_id": "m-2" }),
    });
    daemon.flush_event_sink_worker_for_test();

    let snapshot = daemon.metrics_snapshot();
    assert_eq!(
        snapshot["counters"]["sdk_event_sink_error_total"],
        json!(baseline_errors.saturating_add(1)),
        "failed sink delivery should increment error counter"
    );
}

#[test]
fn sdk_event_sink_bridge_dispatch_does_not_block_on_slow_sink() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let sink: Arc<dyn EventSinkBridge> = Arc::new(SlowFirstSink::new(started_tx, release_rx));
    let daemon = RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        "sink-node".to_string(),
        None,
        None,
        vec![sink],
    );

    let configure = daemon
        .handle_rpc(rpc_request(
            5004,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "allow_kinds": ["webhook"]
                    }
                }
            }),
        ))
        .expect("configure");
    assert!(configure.error.is_none());
    daemon.flush_event_sink_worker_for_test();

    daemon.emit_event(RpcEvent {
        event_type: "delivery_update".to_string(),
        payload: json!({ "message_id": "m-slow-1" }),
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("sink worker should start first publish");

    let started = Instant::now();
    daemon.emit_event(RpcEvent {
        event_type: "delivery_update".to_string(),
        payload: json!({ "message_id": "m-slow-2" }),
    });
    assert!(
        started.elapsed() < Duration::from_millis(20),
        "event publishing should enqueue instead of waiting for the sink worker"
    );

    release_tx.send(()).expect("release slow sink");
    daemon.flush_event_sink_worker_for_test();
}

#[test]
fn ble_and_capture_metrics_are_exposed_in_snapshot() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon =
        RpcDaemon::with_store_and_bridges(store, "metrics-node".to_string(), None, None);

    daemon.metrics_record_ble_connect_failure("ble-main");
    daemon.metrics_record_ble_chunk_retry("ble-main", "ack_timeout");
    daemon.metrics_record_ble_nack("ble-main");
    daemon.metrics_record_ble_tx_queue_timeout("ble-main");
    daemon.metrics_record_attachment_upload_offset_reject("SDK_RUNTIME_INVALID_CURSOR");
    daemon.metrics_record_attachment_upload_checksum_mismatch();
    daemon.metrics_record_capture_success("esp32-cam-1");
    daemon.metrics_record_capture_failure("esp32-cam-1", "disconnect");

    let snapshot = daemon.metrics_snapshot();
    assert_eq!(snapshot["counters"]["ble_connect_failures_total"], json!(1));
    assert_eq!(snapshot["counters"]["ble_chunk_retries_total"], json!(1));
    assert_eq!(snapshot["counters"]["ble_nacks_total"], json!(1));
    assert_eq!(snapshot["counters"]["ble_tx_queue_timeout_total"], json!(1));
    assert_eq!(snapshot["counters"]["attachment_upload_offset_reject_total"], json!(1));
    assert_eq!(snapshot["counters"]["attachment_upload_checksum_mismatch_total"], json!(1));
    assert_eq!(snapshot["counters"]["capture_success_total"], json!(1));
    assert_eq!(snapshot["counters"]["capture_failure_total"], json!(1));
    assert_eq!(snapshot["ble_connect_failures_by_iface"]["ble-main"], json!(1));
    assert_eq!(
        snapshot["ble_chunk_retries_by_iface_reason"]["ble-main|ack_timeout"],
        json!(1)
    );
    assert_eq!(snapshot["capture_success_by_camera_id"]["esp32-cam-1"], json!(1));
    assert_eq!(
        snapshot["capture_failure_by_camera_reason"]["esp32-cam-1|disconnect"],
        json!(1)
    );
}

struct RecoveringReceiptSink {
    attempts: std::sync::atomic::AtomicUsize,
    envelopes: Arc<Mutex<Vec<RpcEventSinkEnvelope>>>,
}

impl EventSinkBridge for RecoveringReceiptSink {
    fn sink_id(&self) -> &str {
        "recovering-receipt"
    }
    fn sink_kind(&self) -> &'static str {
        "custom"
    }
    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> std::io::Result<()> {
        if envelope.event.event_type != "receipt" {
            return Ok(());
        }
        match self.attempts.fetch_add(1, Ordering::SeqCst) {
            0 => return Err(std::io::Error::other("notification rejected")),
            1 => panic!("notification callback panicked"),
            _ => {}
        }
        self.envelopes.lock().expect("recovered sink").push(envelope.clone());
        Ok(())
    }
}

#[test]
fn sdk_event_sink_recovers_after_error_and_panic_without_rewriting_terminal_receipts() {
    let dir = tempfile::tempdir().expect("temporary store");
    let path = dir.path().join("messages.db");
    let store = MessagesStore::open(&path).expect("persistent store");
    let healthy = Arc::new(Mutex::new(Vec::new()));
    let recovered = Arc::new(Mutex::new(Vec::new()));
    let daemon = RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        "callback-recovery".into(),
        None,
        None,
        vec![
            Arc::new(RecoveringReceiptSink {
                attempts: std::sync::atomic::AtomicUsize::new(0),
                envelopes: recovered.clone(),
            }),
            Arc::new(RecordingSink::new("healthy", "custom", false, healthy.clone())),
        ],
    );
    let config = daemon
        .handle_rpc(rpc_request(
            5100,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {"event_sink": {"enabled": true, "allow_kinds": ["custom"]}}
            }),
        ))
        .expect("configure");
    assert!(config.error.is_none());
    daemon.flush_event_sink_worker_for_test();
    healthy.lock().expect("healthy").clear();
    for id in ["failed-error", "failed-panic", "later-delivery"] {
        daemon
            .store
            .insert_message(&MessageRecord {
                id: id.into(),
                source: "src".into(),
                destination: "dst".into(),
                title: String::new(),
                content: "body".into(),
                timestamp: 1,
                direction: "out".into(),
                fields: None,
                receipt_status: Some("sending".into()),
            })
            .expect("persist outgoing message");
        let status = if id == "later-delivery" { "delivered" } else { "failed: no path" };
        let result =
            daemon.record_network_receipt(id, status, JsonMap::new()).expect("commit receipt");
        assert_eq!(result["updated"], json!(true));
        daemon.flush_event_sink_worker_for_test();
        assert_eq!(
            daemon.message_receipt_status(id).expect("stored status").as_deref(),
            Some(status)
        );
    }
    let repeated = daemon
        .record_network_receipt("failed-panic", "delivered", JsonMap::new())
        .expect("late receipt");
    assert_eq!(repeated["updated"], json!(false));
    assert_eq!(repeated["status"], json!("failed: no path"));
    daemon.flush_event_sink_worker_for_test();
    let healthy = healthy.lock().expect("healthy");
    let receipts: Vec<_> =
        healthy.iter().filter(|event| event.event.event_type == "receipt").collect();
    assert_eq!(receipts.len(), 4, "healthy sink receives both failed and subsequent receipts");
    assert_eq!(
        receipts.iter().filter(|event| event.event.payload["updated"] == json!(true)).count(),
        3
    );
    let recovered = recovered.lock().expect("recovered");
    assert_eq!(recovered.len(), 2, "the same callback can handle later notifications");
    assert_eq!(recovered[0].event.payload["message_id"], json!("later-delivery"));
    let snapshot = daemon.metrics_snapshot();
    assert_eq!(snapshot["counters"]["sdk_event_sink_error_total"], json!(2));
    assert_eq!(snapshot["sdk_event_sink_errors_by_kind"]["custom"], json!(2));
    drop(healthy);
    drop(recovered);
    drop(daemon);
    let reopened = MessagesStore::open(&path).expect("reopen persisted store");
    for (id, expected) in [
        ("failed-error", "failed: no path"),
        ("failed-panic", "failed: no path"),
        ("later-delivery", "delivered"),
    ] {
        assert_eq!(
            reopened.get_message(id).expect("read").expect("message").receipt_status.as_deref(),
            Some(expected)
        );
    }
}
