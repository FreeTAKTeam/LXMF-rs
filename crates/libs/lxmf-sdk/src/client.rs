#[cfg(feature = "sdk-async")]
use crate::api::LxmfSdkAsync;
use crate::api::{
    LxmfSdk, LxmfSdkAttachments, LxmfSdkManualTick, LxmfSdkMarkers, LxmfSdkTelemetry, LxmfSdkTopics,
};
use crate::backend::SdkBackend;
#[cfg(feature = "sdk-async")]
use crate::backend::SdkBackendAsyncEvents;
use crate::capability::{NegotiationRequest, NegotiationResponse};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::lifecycle::{Lifecycle, SdkMethod};
use crate::profiles::required_capabilities;
use crate::types::{
    Ack, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot, MessageId, Profile,
    RuntimeSnapshot, RuntimeState, SendRequest, ShutdownMode, StartRequest, TickBudget, TickResult,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Instant;

struct IdempotencyRecord {
    payload_hash: u64,
    message_id: MessageId,
    seen_at: Instant,
}

pub struct Client<B: SdkBackend> {
    backend: B,
    lifecycle: Mutex<Lifecycle>,
    handle: Mutex<Option<ClientHandle>>,
    idempotency_cache: Mutex<HashMap<(String, String, String), IdempotencyRecord>>,
}

impl<B: SdkBackend> Client<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            lifecycle: Mutex::new(Lifecycle::default()),
            handle: Mutex::new(None),
            idempotency_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    fn ensure_capabilities(
        profile: Profile,
        requested_capabilities: &[String],
        negotiation: &NegotiationResponse,
    ) -> Result<(), SdkError> {
        let mut expected = required_capabilities(profile)
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect::<Vec<_>>();
        expected.extend(requested_capabilities.iter().cloned());

        for capability in expected {
            let normalized = capability.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            if !negotiation
                .effective_capabilities
                .iter()
                .any(|value| value.eq_ignore_ascii_case(normalized.as_str()))
            {
                return Err(SdkError::new(
                    code::CAPABILITY_CONTRACT_INCOMPATIBLE,
                    ErrorCategory::Capability,
                    format!("missing required capability '{normalized}' after negotiation"),
                )
                .with_user_actionable(true));
            }
        }
        Ok(())
    }

    fn rollback_start_transition(&self) {
        let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
        if lifecycle.state() == RuntimeState::Starting {
            lifecycle.reset_to_new();
        }
    }

    fn payload_hash(payload: &serde_json::Value) -> Result<u64, SdkError> {
        let serialized = serde_json::to_string(payload).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serialized.hash(&mut hasher);
        Ok(hasher.finish())
    }

    fn current_limits(&self) -> Option<crate::capability::EffectiveLimits> {
        self.handle
            .lock()
            .expect("client handle mutex poisoned")
            .as_ref()
            .map(|handle| handle.effective_limits.clone())
    }

    fn as_client_handle(negotiation: NegotiationResponse) -> ClientHandle {
        ClientHandle {
            runtime_id: negotiation.runtime_id,
            active_contract_version: negotiation.active_contract_version,
            effective_capabilities: negotiation.effective_capabilities,
            effective_limits: negotiation.effective_limits,
        }
    }
}

impl<B: SdkBackend> LxmfSdk for Client<B> {
    fn start(&self, req: StartRequest) -> Result<ClientHandle, SdkError> {
        req.validate()?;

        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if lifecycle.check_start_reentry(&req)? {
                return self
                    .handle
                    .lock()
                    .expect("client handle mutex poisoned")
                    .clone()
                    .ok_or_else(|| {
                        SdkError::new(
                            code::INTERNAL,
                            ErrorCategory::Internal,
                            "runtime is running but client handle is missing",
                        )
                    });
            }
        }

        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.mark_starting()?;
        }

        let negotiation = match self.backend.negotiate(NegotiationRequest {
            supported_contract_versions: req.supported_contract_versions.clone(),
            requested_capabilities: req.requested_capabilities.clone(),
            profile: req.config.profile.clone(),
            bind_mode: req.config.bind_mode.clone(),
            auth_mode: req.config.auth_mode.clone(),
            overflow_policy: req.config.overflow_policy.clone(),
            block_timeout_ms: req.config.block_timeout_ms,
            rpc_backend: req.config.rpc_backend.clone(),
        }) {
            Ok(negotiation) => negotiation,
            Err(err) => {
                self.rollback_start_transition();
                return Err(err);
            }
        };
        if let Err(err) = Self::ensure_capabilities(
            req.config.profile.clone(),
            &req.requested_capabilities,
            &negotiation,
        ) {
            self.rollback_start_transition();
            return Err(err);
        }
        let handle = Self::as_client_handle(negotiation);
        {
            let mut guard = self.handle.lock().expect("client handle mutex poisoned");
            *guard = Some(handle.clone());
        }

        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if let Err(err) = lifecycle.mark_running(req) {
                let mut guard = self.handle.lock().expect("client handle mutex poisoned");
                *guard = None;
                if lifecycle.state() == RuntimeState::Starting {
                    lifecycle.reset_to_new();
                }
                return Err(err);
            }
        }

        Ok(handle)
    }

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Send)?;
        }

        let Some(idempotency_key) = req.idempotency_key.clone() else {
            return self.backend.send(req);
        };

        let ttl_ms =
            self.current_limits().map(|limits| limits.idempotency_ttl_ms).unwrap_or(86_400_000);
        let now = Instant::now();
        let cache_key = (req.source.clone(), req.destination.clone(), idempotency_key);
        let payload_hash = Self::payload_hash(&req.payload)?;

        let mut cache = self.idempotency_cache.lock().expect("idempotency_cache mutex poisoned");
        cache.retain(|_, record| {
            now.duration_since(record.seen_at).as_millis() <= u128::from(ttl_ms)
        });
        if let Some(existing) = cache.get(&cache_key) {
            if existing.payload_hash == payload_hash {
                return Ok(existing.message_id.clone());
            }
            return Err(SdkError::new(
                code::VALIDATION_IDEMPOTENCY_CONFLICT,
                ErrorCategory::Validation,
                "idempotency key already used for different payload",
            )
            .with_user_actionable(true));
        }

        let message_id = self.backend.send(req)?;
        cache.insert(
            cache_key,
            IdempotencyRecord { payload_hash, message_id: message_id.clone(), seen_at: now },
        );
        Ok(message_id)
    }

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Cancel)?;
        }
        self.backend.cancel(id)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Status)?;
        }
        self.backend.status(id)
    }

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError> {
        if patch.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "config patch must contain at least one key",
            )
            .with_user_actionable(true));
        }
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Configure)?;
        }
        self.backend.configure(expected_revision, patch)
    }

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::PollEvents)?;
        }
        if max == 0 {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "poll max must be greater than zero",
            )
            .with_user_actionable(true));
        }
        if let Some(limits) = self.current_limits() {
            if max > limits.max_poll_events {
                return Err(SdkError::new(
                    code::VALIDATION_MAX_POLL_EVENTS_EXCEEDED,
                    ErrorCategory::Validation,
                    "poll max exceeds negotiated effective_limits.max_poll_events",
                )
                .with_user_actionable(true));
            }
        }
        self.backend.poll_events(cursor, max)
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Snapshot)?;
        }
        self.backend.snapshot()
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let current_state = {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Shutdown)?;
            lifecycle.state()
        };
        if current_state == RuntimeState::Stopped {
            return Ok(Ack { accepted: true, revision: None });
        }
        let ack = self.backend.shutdown(mode)?;
        {
            let mut lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            if lifecycle.state() != RuntimeState::Stopped {
                let _ = lifecycle.mark_draining();
                lifecycle.mark_stopped();
            }
        }
        Ok(ack)
    }
}

impl<B: SdkBackend> LxmfSdkManualTick for Client<B> {
    fn tick(&self, budget: TickBudget) -> Result<TickResult, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Tick)?;
        }
        self.backend.tick(budget)
    }
}

impl<B: SdkBackend> LxmfSdkTopics for Client<B> {}

impl<B: SdkBackend> LxmfSdkTelemetry for Client<B> {}

impl<B: SdkBackend> LxmfSdkAttachments for Client<B> {}

impl<B: SdkBackend> LxmfSdkMarkers for Client<B> {}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> LxmfSdkAsync for Client<B> {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::SubscribeEvents)?;
        }
        self.backend.subscribe_events(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::EffectiveLimits;
    use crate::event::{SdkEvent, Severity};
    use crate::types::{
        AuthMode, BindMode, EventStreamConfig, OverflowPolicy, Profile, RedactionConfig,
        RedactionTransform, SdkConfig, ShutdownMode,
    };
    use serde_json::json;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockBackend {
        negotiate_results: Mutex<VecDeque<Result<NegotiationResponse, SdkError>>>,
        shutdown_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
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
                shutdown_calls: AtomicUsize::new(0),
            }
        }

        fn with_shutdown_results(mut self, results: Vec<Result<Ack, SdkError>>) -> Self {
            self.shutdown_results = Mutex::new(VecDeque::from(results));
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
            Ok(MessageId("m-1".to_owned()))
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
                event_stream: EventStreamConfig {
                    max_poll_events: 256,
                    max_event_bytes: 65_536,
                    max_batch_bytes: 1_048_576,
                    max_extension_keys: 32,
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
}
