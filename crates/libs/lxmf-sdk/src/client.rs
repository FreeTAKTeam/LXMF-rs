#[cfg(feature = "sdk-async")]
use crate::api::LxmfSdkAsync;
use crate::api::{
    LxmfSdk, LxmfSdkAttachments, LxmfSdkGroupDelivery, LxmfSdkIdentity, LxmfSdkManualTick,
    LxmfSdkMarkers, LxmfSdkPaper, LxmfSdkRemoteCommands, LxmfSdkTelemetry, LxmfSdkTopics,
    LxmfSdkVoiceSignaling,
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
use crate::profiles::{required_capabilities, supports_capability};
use crate::types::{
    Ack, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot, GroupRecipientState,
    GroupSendOutcome, GroupSendRequest, GroupSendResult, MessageId, Profile, RuntimeSnapshot,
    RuntimeState, SendRequest, ShutdownMode, StartRequest, TickBudget, TickResult,
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

#[path = "client/domains.rs"]
mod domains;

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
        let mut expected = required_capabilities(profile.clone())
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect::<Vec<_>>();
        for capability in requested_capabilities {
            let normalized = capability.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            // Unknown/future capability IDs are treated as optional hints. Only enforce
            // requested capabilities that this profile can actually support.
            if supports_capability(profile.clone(), normalized.as_str()) {
                expected.push(normalized);
            }
        }

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

impl<B: SdkBackend> LxmfSdkGroupDelivery for Client<B> {
    fn send_group(&self, req: GroupSendRequest) -> Result<GroupSendResult, SdkError> {
        {
            let lifecycle = self.lifecycle.lock().expect("lifecycle mutex poisoned");
            lifecycle.ensure_method_legal(SdkMethod::Send)?;
        }

        let source = req.source.trim();
        if source.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "group send source must not be empty",
            )
            .with_user_actionable(true));
        }
        if req.destinations.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "group send requires at least one destination",
            )
            .with_user_actionable(true));
        }

        let mut outcomes = Vec::with_capacity(req.destinations.len());
        for destination in req.destinations {
            let trimmed_destination = destination.trim().to_owned();
            if trimmed_destination.is_empty() {
                outcomes.push(GroupSendOutcome {
                    destination,
                    state: GroupRecipientState::Failed,
                    message_id: None,
                    retryable: false,
                    reason_code: Some(code::VALIDATION_INVALID_ARGUMENT.to_owned()),
                });
                continue;
            }

            let send_request = SendRequest {
                source: source.to_owned(),
                destination: trimmed_destination.clone(),
                payload: req.payload.clone(),
                idempotency_key: req.idempotency_key.clone(),
                ttl_ms: req.ttl_ms,
                correlation_id: req.correlation_id.clone(),
                extensions: req.extensions.clone(),
            };
            match self.send(send_request) {
                Ok(message_id) => outcomes.push(GroupSendOutcome {
                    destination: trimmed_destination,
                    state: GroupRecipientState::Accepted,
                    message_id: Some(message_id),
                    retryable: false,
                    reason_code: None,
                }),
                Err(err) => {
                    let state = if err.is_retryable() {
                        GroupRecipientState::Deferred
                    } else {
                        GroupRecipientState::Failed
                    };
                    outcomes.push(GroupSendOutcome {
                        destination: trimmed_destination,
                        state,
                        message_id: None,
                        retryable: err.is_retryable(),
                        reason_code: Some(err.machine_code),
                    });
                }
            }
        }

        let accepted_count = outcomes
            .iter()
            .filter(|outcome| outcome.state == GroupRecipientState::Accepted)
            .count();
        let deferred_count = outcomes
            .iter()
            .filter(|outcome| outcome.state == GroupRecipientState::Deferred)
            .count();
        let failed_count =
            outcomes.iter().filter(|outcome| outcome.state == GroupRecipientState::Failed).count();

        Ok(GroupSendResult { outcomes, accepted_count, deferred_count, failed_count })
    }
}

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
#[path = "client/tests.rs"]
mod tests;
