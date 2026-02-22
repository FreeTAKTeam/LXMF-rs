use std::sync::{Arc, Mutex};

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
    envelopes.lock().expect("envelopes mutex poisoned").clear();

    daemon.emit_event(RpcEvent {
        event_type: "security_notice".to_string(),
        payload: json!({
            "token": "top-secret-token",
            "peer_id": "peer-1",
            "message": "ok"
        }),
    });

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
    webhook_envelopes
        .lock()
        .expect("webhook envelopes mutex")
        .clear();
    mqtt_envelopes.lock().expect("mqtt envelopes mutex").clear();

    daemon.emit_event(RpcEvent {
        event_type: "delivery_update".to_string(),
        payload: json!({ "message_id": "m-1" }),
    });

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

    let snapshot = daemon.metrics_snapshot();
    assert_eq!(
        snapshot["counters"]["sdk_event_sink_error_total"],
        json!(baseline_errors.saturating_add(1)),
        "failed sink delivery should increment error counter"
    );
}
