use super::*;
use crate::api::LxmfSdkGroupDelivery;
use crate::capability::EffectiveLimits;
use crate::event::{SdkEvent, Severity};
use crate::types::{
    AuthMode, BindMode, EventSinkConfig, EventSinkKind, EventStreamConfig, GroupRecipientState,
    GroupSendRequest, OverflowPolicy, Profile, RedactionConfig, RedactionTransform, SdkConfig,
    ShutdownMode, StoreForwardCapacityPolicy, StoreForwardConfig, StoreForwardEvictionPriority,
};
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct MockBackend {
    negotiate_results: Mutex<VecDeque<Result<NegotiationResponse, SdkError>>>,
    shutdown_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
    send_results: Mutex<VecDeque<Result<MessageId, SdkError>>>,
    send_calls: AtomicUsize,
    shutdown_calls: AtomicUsize,
}

impl MockBackend {
    fn new(negotiate_results: Vec<Result<NegotiationResponse, SdkError>>) -> Self {
        Self {
            negotiate_results: Mutex::new(VecDeque::from(negotiate_results)),
            shutdown_results: Mutex::new(VecDeque::from(vec![Ok(Ack {
                accepted: true,
                revision: None,
            })])),
            send_results: Mutex::new(VecDeque::new()),
            send_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
        }
    }

    fn with_shutdown_results(mut self, results: Vec<Result<Ack, SdkError>>) -> Self {
        self.shutdown_results = Mutex::new(VecDeque::from(results));
        self
    }

    fn with_send_results(mut self, results: Vec<Result<MessageId, SdkError>>) -> Self {
        self.send_results = Mutex::new(VecDeque::from(results));
        self
    }
}

impl SdkBackend for MockBackend {
    fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        self.negotiate_results
            .lock()
            .expect("negotiate_results mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(SdkError::new(
                    code::INTERNAL,
                    ErrorCategory::Internal,
                    "no negotiate response queued",
                ))
            })
    }

    fn send(&self, _req: SendRequest) -> Result<MessageId, SdkError> {
        let sequence = self.send_calls.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(result) =
            self.send_results.lock().expect("send_results mutex poisoned").pop_front()
        {
            return result;
        }
        Ok(MessageId(format!("m-{sequence}")))
    }

    fn cancel(&self, _id: MessageId) -> Result<CancelResult, SdkError> {
        Ok(CancelResult::Accepted)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        Ok(Some(DeliverySnapshot {
            message_id: id,
            state: crate::types::DeliveryState::Queued,
            terminal: false,
            last_updated_ms: 0,
            attempts: 0,
            reason_code: None,
        }))
    }

    fn configure(&self, _expected_revision: u64, _patch: ConfigPatch) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: Some(1) })
    }

    fn poll_events(
        &self,
        cursor: Option<EventCursor>,
        _max: usize,
    ) -> Result<EventBatch, SdkError> {
        Ok(EventBatch {
            events: vec![SdkEvent {
                event_id: "evt-1".to_owned(),
                runtime_id: "rt-1".to_owned(),
                stream_id: "stream".to_owned(),
                seq_no: 1,
                contract_version: 2,
                ts_ms: 1,
                event_type: "RuntimeStateChanged".to_owned(),
                severity: Severity::Info,
                source_component: "test".to_owned(),
                operation_id: None,
                message_id: None,
                peer_id: None,
                correlation_id: None,
                trace_id: None,
                payload: json!({}),
                extensions: BTreeMap::new(),
            }],
            next_cursor: cursor.unwrap_or_else(|| EventCursor("cursor-1".to_owned())),
            dropped_count: 0,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        })
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        Ok(RuntimeSnapshot {
            runtime_id: "rt-1".to_owned(),
            state: RuntimeState::Running,
            active_contract_version: 2,
            event_stream_position: 1,
            config_revision: 0,
            queued_messages: 0,
            in_flight_messages: 0,
        })
    }

    fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
        self.shutdown_calls.fetch_add(1, Ordering::Relaxed);
        self.shutdown_results
            .lock()
            .expect("shutdown_results mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(SdkError::new(
                    code::INTERNAL,
                    ErrorCategory::Internal,
                    "no shutdown response queued",
                ))
            })
    }

    fn tick(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
        Ok(TickResult { processed_items: 0, yielded: false, next_recommended_delay_ms: None })
    }
}

fn sample_start_request() -> StartRequest {
    StartRequest {
        supported_contract_versions: vec![2],
        requested_capabilities: vec!["sdk.capability.cursor_replay".to_owned()],
        config: SdkConfig {
            profile: Profile::DesktopFull,
            bind_mode: BindMode::LocalOnly,
            auth_mode: AuthMode::LocalTrusted,
            overflow_policy: OverflowPolicy::Reject,
            block_timeout_ms: None,
            store_forward: StoreForwardConfig {
                max_messages: 50_000,
                max_message_age_ms: 604_800_000,
                capacity_policy: StoreForwardCapacityPolicy::DropOldest,
                eviction_priority: StoreForwardEvictionPriority::TerminalFirst,
            },
            event_stream: EventStreamConfig {
                max_poll_events: 256,
                max_event_bytes: 65_536,
                max_batch_bytes: 1_048_576,
                max_extension_keys: 32,
            },
            event_sink: EventSinkConfig {
                enabled: false,
                max_event_bytes: 65_536,
                allow_kinds: vec![
                    EventSinkKind::Webhook,
                    EventSinkKind::Mqtt,
                    EventSinkKind::Custom,
                ],
                extensions: BTreeMap::new(),
            },
            idempotency_ttl_ms: 86_400_000,
            redaction: RedactionConfig {
                enabled: true,
                sensitive_transform: RedactionTransform::Hash,
                break_glass_allowed: false,
                break_glass_ttl_ms: None,
            },
            rpc_backend: None,
            extensions: BTreeMap::new(),
        },
    }
}

fn successful_negotiation() -> Result<NegotiationResponse, SdkError> {
    Ok(NegotiationResponse {
        runtime_id: "rt-1".to_owned(),
        active_contract_version: 2,
        effective_capabilities: vec![
            "sdk.capability.cursor_replay".to_owned(),
            "sdk.capability.async_events".to_owned(),
            "sdk.capability.receipt_terminality".to_owned(),
            "sdk.capability.config_revision_cas".to_owned(),
            "sdk.capability.idempotency_ttl".to_owned(),
        ],
        effective_limits: EffectiveLimits {
            max_poll_events: 256,
            max_event_bytes: 65_536,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
            idempotency_ttl_ms: 86_400_000,
        },
        contract_release: "v2.5".to_owned(),
        schema_namespace: "v2".to_owned(),
    })
}

fn sample_send_request(payload: &str, idempotency_key: Option<&str>) -> SendRequest {
    serde_json::from_value(json!({
        "source": "src",
        "destination": "dst",
        "payload": { "content": payload },
        "idempotency_key": idempotency_key,
        "ttl_ms": null,
        "correlation_id": null,
        "extensions": {},
    }))
    .expect("deserialize send request")
}

#[test]
fn start_failure_rolls_back_to_new_and_can_retry() {
    let backend = MockBackend::new(vec![
        Err(SdkError::new(code::INTERNAL, ErrorCategory::Transport, "dial failure")),
        successful_negotiation(),
    ]);
    let client = Client::new(backend);

    let first = client.start(sample_start_request());
    assert!(first.is_err());
    let second = client.start(sample_start_request());
    assert!(second.is_ok());
}

#[test]
fn start_ignores_unknown_requested_capabilities_when_known_overlap_exists() {
    let backend = MockBackend::new(vec![successful_negotiation()]);
    let client = Client::new(backend);
    let mut request = sample_start_request();
    request.requested_capabilities = vec![
        "sdk.capability.cursor_replay".to_owned(),
        "sdk.capability.future_contract_extension".to_owned(),
    ];

    let handle = client.start(request).expect("unknown requested capability should be ignored");
    assert_eq!(handle.active_contract_version, 2);
    assert!(handle
        .effective_capabilities
        .iter()
        .any(|capability| capability == "sdk.capability.cursor_replay"));
}

#[test]
fn shutdown_is_noop_once_stopped() {
    let backend = MockBackend::new(vec![successful_negotiation()]).with_shutdown_results(vec![
        Ok(Ack { accepted: true, revision: None }),
        Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Transport,
            "backend shutdown should not be called again",
        )),
    ]);
    let client = Client::new(backend);
    client.start(sample_start_request()).expect("start");
    let first = client.shutdown(ShutdownMode::Graceful).expect("first shutdown");
    assert!(first.accepted);
    let second = client.shutdown(ShutdownMode::Graceful).expect("second shutdown must be noop");
    assert!(second.accepted);
    assert_eq!(client.backend().shutdown_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn race_idempotent_send_parallel_calls_dedupe_to_single_backend_send() {
    let backend = MockBackend::new(vec![successful_negotiation()]);
    let client = Arc::new(Client::new(backend));
    client.start(sample_start_request()).expect("start");

    let mut workers = Vec::new();
    for _ in 0..24 {
        let client = Arc::clone(&client);
        workers.push(std::thread::spawn(move || {
            client.send(sample_send_request("payload-race", Some("idem-race")))
        }));
    }

    let mut first: Option<MessageId> = None;
    for worker in workers {
        let result = worker.join().expect("worker panicked").expect("send result");
        match &first {
            Some(expected) => assert_eq!(&result, expected, "all idempotent sends must reuse id"),
            None => first = Some(result),
        }
    }

    assert_eq!(
        client.backend().send_calls.load(Ordering::Relaxed),
        1,
        "parallel idempotent sends must issue a single backend send"
    );
}

#[test]
fn race_idempotency_conflict_parallel_payloads_return_conflict() {
    let backend = MockBackend::new(vec![successful_negotiation()]);
    let client = Arc::new(Client::new(backend));
    client.start(sample_start_request()).expect("start");

    let mut workers = Vec::new();
    for idx in 0..24 {
        let client = Arc::clone(&client);
        workers.push(std::thread::spawn(move || {
            let payload = if idx % 2 == 0 { "payload-a" } else { "payload-b" };
            client.send(sample_send_request(payload, Some("idem-conflict")))
        }));
    }

    let mut success = 0_usize;
    let mut conflicts = 0_usize;
    for worker in workers {
        match worker.join().expect("worker panicked") {
            Ok(_) => success = success.saturating_add(1),
            Err(err) if err.machine_code == code::VALIDATION_IDEMPOTENCY_CONFLICT => {
                conflicts = conflicts.saturating_add(1);
            }
            Err(err) => panic!("unexpected send error: {err:?}"),
        }
    }

    assert!(success > 0, "at least one send must succeed");
    assert!(conflicts > 0, "conflicting payloads must produce idempotency conflicts");
    assert_eq!(
        client.backend().send_calls.load(Ordering::Relaxed),
        1,
        "idempotency conflict races must not duplicate backend sends"
    );
}

#[test]
fn group_send_returns_partial_outcomes_with_retry_classification() {
    let retryable = SdkError::new(code::INTERNAL, ErrorCategory::Transport, "temporary failure")
        .with_retryable(true);
    let hard_fail = SdkError::new(
        code::VALIDATION_INVALID_ARGUMENT,
        ErrorCategory::Validation,
        "invalid destination",
    );
    let backend = MockBackend::new(vec![successful_negotiation()]).with_send_results(vec![
        Ok(MessageId("m-1".to_owned())),
        Err(retryable),
        Err(hard_fail),
    ]);
    let client = Client::new(backend);
    client.start(sample_start_request()).expect("start");

    let result = client
        .send_group(GroupSendRequest::new(
            "src",
            vec!["dst-a", "dst-b", "dst-c"],
            json!({ "content": "group hello" }),
        ))
        .expect("group send should return outcomes");

    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.deferred_count, 1);
    assert_eq!(result.failed_count, 1);
    assert_eq!(result.outcomes.len(), 3);
    assert_eq!(result.outcomes[0].state, GroupRecipientState::Accepted);
    assert_eq!(result.outcomes[0].message_id, Some(MessageId("m-1".to_owned())));
    assert_eq!(result.outcomes[1].state, GroupRecipientState::Deferred);
    assert!(result.outcomes[1].retryable);
    assert_eq!(result.outcomes[1].reason_code.as_deref(), Some(code::INTERNAL));
    assert_eq!(result.outcomes[2].state, GroupRecipientState::Failed);
    assert_eq!(result.outcomes[2].reason_code.as_deref(), Some(code::VALIDATION_INVALID_ARGUMENT));
}

#[test]
fn group_send_rejects_empty_destination_list() {
    let backend = MockBackend::new(vec![successful_negotiation()]);
    let client = Client::new(backend);
    client.start(sample_start_request()).expect("start");

    let err = client
        .send_group(GroupSendRequest::new("src", Vec::<String>::new(), json!({ "content": "x" })))
        .expect_err("group send without destinations must fail");
    assert_eq!(err.machine_code, code::VALIDATION_INVALID_ARGUMENT);
}
