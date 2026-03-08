use super::capabilities::CapabilitySummary;
use super::delivery::{
    AttemptDecision, AttemptDisposition, DeliveryAttempt, DeliveryOptions, DeliveryPlan,
    SendReport,
};
use super::errors::Error;
#[cfg(feature = "sdk-async")]
use super::events::{
    map_event_batch, subscription_cursor, EventBatch, SubscriptionStart,
};
use crate::{
    Client as CoreClient, ClientHandle, DeliverySnapshot, DeliveryState as RawDeliveryState,
    EventCursor, LxmfSdk, Profile as CoreProfile, RuntimeSnapshot, RuntimeState, SdkBackend,
    SdkConfig, SendRequest as RawSendRequest, ShutdownMode, StartRequest,
};
#[cfg(feature = "sdk-async")]
use crate::{LxmfSdkAsync, SdkBackendAsyncEvents};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Profile {
    MobileDefault,
    DesktopDefault,
    EmbeddedDefault,
    TestingDefault,
}

impl Profile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MobileDefault => "mobile_default",
            Self::DesktopDefault => "desktop_default",
            Self::EmbeddedDefault => "embedded_default",
            Self::TestingDefault => "testing_default",
        }
    }

    pub fn to_sdk_profile(&self) -> CoreProfile {
        match self {
            Self::MobileDefault => CoreProfile::DesktopLocalRuntime,
            Self::DesktopDefault => CoreProfile::DesktopFull,
            Self::EmbeddedDefault => CoreProfile::EmbeddedAlloc,
            Self::TestingDefault => CoreProfile::DesktopLocalRuntime,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Config {
    pub profile: Profile,
    pub sdk_config: SdkConfig,
    pub supported_contract_versions: Vec<u16>,
    pub requested_capabilities: Vec<String>,
    pub event_batch_size: Option<usize>,
}

impl Config {
    pub fn mobile_default() -> Self {
        Self::from_profile(Profile::MobileDefault)
    }

    pub fn desktop_default() -> Self {
        Self::from_profile(Profile::DesktopDefault)
    }

    pub fn embedded_default() -> Self {
        Self::from_profile(Profile::EmbeddedDefault)
    }

    pub fn testing_default() -> Self {
        Self::from_profile(Profile::TestingDefault)
    }

    pub fn with_requested_capability(mut self, capability: impl Into<String>) -> Self {
        self.requested_capabilities.push(capability.into());
        self
    }

    pub fn start_request(&self) -> StartRequest {
        StartRequest::new(self.sdk_config.clone())
            .with_supported_contract_versions(self.supported_contract_versions.clone())
            .with_requested_capabilities(self.requested_capabilities.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RunState {
    New,
    Starting,
    Running,
    Degraded,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Handle {
    pub runtime_id: String,
    pub profile: Profile,
    pub capabilities: CapabilitySummary,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct RuntimeStatus {
    pub runtime_id: Option<String>,
    pub state: RunState,
    pub profile: Option<Profile>,
    pub capabilities: Option<CapabilitySummary>,
    pub queued_messages: u64,
    pub in_flight_messages: u64,
    pub event_stream_position: u64,
    pub config_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendRequest {
    pub source: String,
    pub destination: String,
    pub payload: JsonValue,
    pub idempotency_key: Option<String>,
    pub ttl_ms: Option<u64>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl SendRequest {
    pub fn new(
        source: impl Into<String>,
        destination: impl Into<String>,
        payload: JsonValue,
    ) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            payload,
            idempotency_key: None,
            ttl_ms: None,
            correlation_id: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = Some(ttl_ms);
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    fn into_raw(self) -> RawSendRequest {
        RawSendRequest {
            source: self.source,
            destination: self.destination,
            payload: self.payload,
            idempotency_key: self.idempotency_key,
            ttl_ms: self.ttl_ms,
            correlation_id: self.correlation_id,
            extensions: self.extensions,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendReceipt {
    pub runtime_id: String,
    pub message_id: String,
    pub profile: Profile,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeliveryState {
    Queued,
    Dispatching,
    Sent,
    Delivered,
    Failed,
    Cancelled,
    Expired,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct DeliveryStatus {
    pub message_id: String,
    pub state: DeliveryState,
    pub terminal: bool,
    pub last_updated_ms: u64,
    pub attempts: u32,
    pub reason_code: Option<String>,
}

#[derive(Clone)]
struct SharedBackend<B>(Arc<B>);

impl<B: SdkBackend> SdkBackend for SharedBackend<B> {
    fn negotiate(
        &self,
        req: crate::NegotiationRequest,
    ) -> Result<crate::NegotiationResponse, crate::SdkError> {
        self.0.negotiate(req)
    }

    fn send(&self, req: RawSendRequest) -> Result<crate::MessageId, crate::SdkError> {
        self.0.send(req)
    }

    fn cancel(&self, id: crate::MessageId) -> Result<crate::CancelResult, crate::SdkError> {
        self.0.cancel(id)
    }

    fn status(&self, id: crate::MessageId) -> Result<Option<DeliverySnapshot>, crate::SdkError> {
        self.0.status(id)
    }

    fn configure(
        &self,
        expected_revision: u64,
        patch: crate::ConfigPatch,
    ) -> Result<crate::Ack, crate::SdkError> {
        self.0.configure(expected_revision, patch)
    }

    fn poll_events(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<crate::EventBatch, crate::SdkError> {
        self.0.poll_events(cursor, max)
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, crate::SdkError> {
        self.0.snapshot()
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<crate::Ack, crate::SdkError> {
        self.0.shutdown(mode)
    }

    fn tick(&self, budget: crate::TickBudget) -> Result<crate::TickResult, crate::SdkError> {
        self.0.tick(budget)
    }
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> SdkBackendAsyncEvents for SharedBackend<B> {
    fn subscribe_events(
        &self,
        start: crate::SubscriptionStart,
    ) -> Result<crate::EventSubscription, crate::SdkError> {
        self.0.subscribe_events(start)
    }
}

struct SessionState {
    handle: ClientHandle,
    config: Config,
    degraded: bool,
}

struct State<B: SdkBackend> {
    client: Option<Arc<CoreClient<SharedBackend<B>>>>,
    session: Option<Arc<Mutex<SessionState>>>,
    lifecycle: RunState,
}

impl<B: SdkBackend> Default for State<B> {
    fn default() -> Self {
        Self { client: None, session: None, lifecycle: RunState::New }
    }
}

pub struct Client<B: SdkBackend> {
    backend: Arc<B>,
    state: Mutex<State<B>>,
}

impl<B: SdkBackend> Client<B> {
    pub fn new(backend: B) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    pub fn from_arc(backend: Arc<B>) -> Self {
        Self { backend, state: Mutex::new(State::default()) }
    }

    fn active_config(&self) -> Result<Config, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let session = session.lock().expect("app session mutex poisoned");
        Ok(session.config.clone())
    }

    pub fn delivery_plan(&self) -> Result<DeliveryPlan, Error> {
        Ok(self.active_config()?.delivery_plan())
    }

    pub fn start(&self, config: Config) -> Result<Handle, Error> {
        let mut state = self.state.lock().expect("app client mutex poisoned");
        if state.client.is_none() && state.session.is_some() {
            state.session = None;
            state.lifecycle = RunState::Stopped;
        }
        if let Some(session) = state.session.as_ref() {
            let session = session.lock().expect("app session mutex poisoned");
            return if session.config == config {
                Ok(Handle {
                    runtime_id: session.handle.runtime_id.clone(),
                    profile: session.config.profile.clone(),
                    capabilities: CapabilitySummary::from(&session.handle),
                })
            } else {
                Err(Error::already_running_different_config())
            };
        }

        state.lifecycle = RunState::Starting;
        let client = Arc::new(CoreClient::new(SharedBackend(Arc::clone(&self.backend))));
        let handle = match client.start(config.start_request()) {
            Ok(handle) => handle,
            Err(err) => {
                state.lifecycle = RunState::New;
                return Err(Error::from(err));
            }
        };
        let session = Arc::new(Mutex::new(SessionState {
            handle: handle.clone(),
            config: config.clone(),
            degraded: false,
        }));
        state.client = Some(Arc::clone(&client));
        state.session = Some(session);
        state.lifecycle = RunState::Running;

        Ok(Handle {
            runtime_id: handle.runtime_id.clone(),
            profile: config.profile,
            capabilities: CapabilitySummary::from(&handle),
        })
    }

    pub fn restart(&self, config: Config) -> Result<Handle, Error> {
        self.stop(ShutdownMode::Immediate)?;
        self.start(config)
    }

    pub fn send(&self, request: SendRequest) -> Result<SendReceipt, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(Error::not_started());
        };
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let session = session.lock().expect("app session mutex poisoned");
        let correlation_id = request.correlation_id.clone();
        let message_id = client.send(request.into_raw()).map_err(Error::from)?;
        Ok(SendReceipt {
            runtime_id: session.handle.runtime_id.clone(),
            message_id: message_id.0,
            profile: session.config.profile.clone(),
            correlation_id,
        })
    }

    pub fn send_with_profile_defaults(&self, request: SendRequest) -> Result<SendReport, Error> {
        self.send_with_options(request, DeliveryOptions::default())
    }

    pub fn send_with_options(
        &self,
        request: SendRequest,
        options: DeliveryOptions,
    ) -> Result<SendReport, Error> {
        let plan = self.delivery_plan()?;
        let resolved = plan.resolve(&options);
        let started = Instant::now();
        let mut attempts: Vec<DeliveryAttempt> = Vec::new();
        let mut request = request;

        loop {
            let attempt_no = attempts.len() as u32 + 1;
            match self.send(request.clone()) {
                Ok(receipt) => {
                    let total_delay_ms =
                        attempts
                            .iter()
                            .filter_map(|attempt: &DeliveryAttempt| attempt.scheduled_delay_ms)
                            .sum::<u64>();
                    return Ok(SendReport { receipt, attempts, total_delay_ms, plan });
                }
                Err(err) => match resolved.classify_failure(
                    attempt_no,
                    &err,
                    started.elapsed().as_millis() as u64,
                ) {
                    AttemptDecision::Retry(delay_ms) => {
                        attempts.push(DeliveryAttempt {
                            attempt: attempt_no,
                            disposition: AttemptDisposition::Retried,
                            error_code: err.code.as_str().to_owned(),
                            retryable: err.retryable,
                            queue_pressure: matches!(
                                err.code,
                                super::errors::ErrorCode::DeliveryQueuePressure
                            ),
                            scheduled_delay_ms: Some(delay_ms),
                        });
                        if delay_ms > 0 {
                            std::thread::sleep(Duration::from_millis(delay_ms));
                        }
                        request = request.clone();
                    }
                    AttemptDecision::Stop(stop_err) | AttemptDecision::Timeout(stop_err) => {
                        attempts.push(DeliveryAttempt {
                            attempt: attempt_no,
                            disposition: AttemptDisposition::Failed,
                            error_code: err.code.as_str().to_owned(),
                            retryable: err.retryable,
                            queue_pressure: matches!(
                                err.code,
                                super::errors::ErrorCode::DeliveryQueuePressure
                            ),
                            scheduled_delay_ms: None,
                        });
                        return Err(stop_err);
                    }
                },
            }
        }
    }

    pub fn delivery_status(
        &self,
        message_id: impl Into<crate::MessageId>,
    ) -> Result<Option<DeliveryStatus>, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(Error::not_started());
        };
        let snapshot = client.status(message_id.into()).map_err(Error::from)?;
        Ok(snapshot.map(map_delivery_snapshot))
    }

    pub fn status(&self) -> Result<RuntimeStatus, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Ok(RuntimeStatus {
                runtime_id: None,
                state: state.lifecycle.clone(),
                profile: None,
                capabilities: None,
                queued_messages: 0,
                in_flight_messages: 0,
                event_stream_position: 0,
                config_revision: 0,
            });
        };
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let snapshot = client.snapshot().map_err(Error::from)?;
        let session = session.lock().expect("app session mutex poisoned");
        Ok(RuntimeStatus {
            runtime_id: Some(snapshot.runtime_id.clone()),
            state: map_runtime_state(snapshot.state, session.degraded),
            profile: Some(session.config.profile.clone()),
            capabilities: Some(CapabilitySummary::from(&session.handle)),
            queued_messages: snapshot.queued_messages,
            in_flight_messages: snapshot.in_flight_messages,
            event_stream_position: snapshot.event_stream_position,
            config_revision: snapshot.config_revision,
        })
    }

    pub fn stop(&self, mode: ShutdownMode) -> Result<(), Error> {
        let mut state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref().cloned() else {
            state.session = None;
            state.lifecycle = RunState::Stopped;
            return Ok(());
        };
        let previous_lifecycle = state.lifecycle.clone();
        state.lifecycle = RunState::Stopping;
        if let Err(err) = client.shutdown(mode) {
            state.lifecycle = previous_lifecycle;
            return Err(Error::from(err));
        }
        state.client = None;
        state.session = None;
        state.lifecycle = RunState::Stopped;
        Ok(())
    }

    #[cfg(feature = "sdk-async")]
    pub fn subscribe_events(
        &self,
        start: SubscriptionStart,
    ) -> Result<EventStream<B>, Error>
    where
        B: SdkBackendAsyncEvents,
    {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(Error::not_started());
        };
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let subscription =
            client.subscribe_events(start.clone().into()).map_err(Error::from)?;
        let session_guard = session.lock().expect("app session mutex poisoned");
        let max_batch_size = session_guard
            .config
            .event_batch_size
            .unwrap_or(session_guard.handle.effective_limits.max_poll_events)
            .min(session_guard.handle.effective_limits.max_poll_events)
            .max(1);
        let profile = session_guard.config.profile.clone();
        drop(session_guard);
        Ok(EventStream {
            client: Arc::clone(client),
            session: Arc::clone(session),
            cursor: subscription_cursor(&subscription),
            max_batch_size,
            profile,
        })
    }
}

#[cfg(feature = "rpc-backend")]
impl Client<crate::RpcBackendClient> {
    pub fn rpc(endpoint: impl Into<String>) -> Self {
        Self::new(crate::RpcBackendClient::new(endpoint.into()))
    }
}

#[cfg(feature = "sdk-async")]
pub struct EventStream<B: SdkBackendAsyncEvents> {
    client: Arc<CoreClient<SharedBackend<B>>>,
    session: Arc<Mutex<SessionState>>,
    cursor: Option<EventCursor>,
    max_batch_size: usize,
    profile: Profile,
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> EventStream<B> {
    pub fn next_batch(&mut self) -> Result<EventBatch, Error> {
        let batch = self
            .client
            .poll_events(self.cursor.clone(), self.max_batch_size)
            .map_err(Error::from)?;
        self.cursor = Some(batch.next_cursor.clone());

        let batch = map_event_batch(batch, self.profile.as_str());
        if batch.dropped_count > 0
            || batch.events.iter().any(|event| {
                matches!(event.kind, super::events::EventKind::StreamGapDetected(_))
            })
        {
            let mut session = self.session.lock().expect("app session mutex poisoned");
            session.degraded = true;
        }
        Ok(batch)
    }

    pub fn reset(&mut self) {
        self.cursor = None;
        let mut session = self.session.lock().expect("app session mutex poisoned");
        session.degraded = false;
    }
}

fn map_runtime_state(state: RuntimeState, degraded: bool) -> RunState {
    if degraded && state == RuntimeState::Running {
        return RunState::Degraded;
    }
    match state {
        RuntimeState::New => RunState::New,
        RuntimeState::Starting => RunState::Starting,
        RuntimeState::Running => RunState::Running,
        RuntimeState::Draining => RunState::Stopping,
        RuntimeState::Stopped => RunState::Stopped,
        RuntimeState::Failed | RuntimeState::Unknown => RunState::Failed,
    }
}

fn map_delivery_snapshot(snapshot: DeliverySnapshot) -> DeliveryStatus {
    let state = match snapshot.state {
        RawDeliveryState::Queued => DeliveryState::Queued,
        RawDeliveryState::Dispatching | RawDeliveryState::InFlight => DeliveryState::Dispatching,
        RawDeliveryState::Sent => DeliveryState::Sent,
        RawDeliveryState::Delivered => DeliveryState::Delivered,
        RawDeliveryState::Failed => DeliveryState::Failed,
        RawDeliveryState::Cancelled => DeliveryState::Cancelled,
        RawDeliveryState::Expired => DeliveryState::Expired,
        RawDeliveryState::Rejected => DeliveryState::Rejected,
        RawDeliveryState::Unknown => DeliveryState::Unknown,
    };
    DeliveryStatus {
        message_id: snapshot.message_id.0,
        state,
        terminal: snapshot.terminal,
        last_updated_ms: snapshot.last_updated_ms,
        attempts: snapshot.attempts,
        reason_code: snapshot.reason_code,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Client, Config, DeliveryState, Profile, RunState, SendRequest, SubscriptionStart,
    };
    use crate::app::DeliveryOptions;
    use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};
    use crate::event::{
        EventBatch as RawEventBatch, EventCursor, EventSubscription, SdkEvent,
        Severity as RawSeverity,
    };
    use crate::{
        Ack, CancelResult, DeliverySnapshot, DeliveryState as RawDeliveryState, EffectiveLimits,
        NegotiationRequest, NegotiationResponse, Profile as CoreProfile, RuntimeSnapshot,
        RuntimeState, SdkBackend, SdkBackendAsyncEvents, SendRequest as RawSendRequest,
        ShutdownMode,
    };
    use serde_json::json;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockBackend {
        runtime_seq: AtomicUsize,
        send_seq: AtomicUsize,
        poll_batches: Mutex<VecDeque<RawEventBatch>>,
        send_results: Mutex<VecDeque<Result<crate::MessageId, SdkError>>>,
        shutdown_calls: AtomicUsize,
        shutdown_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                runtime_seq: AtomicUsize::new(1),
                send_seq: AtomicUsize::new(1),
                poll_batches: Mutex::new(VecDeque::new()),
                send_results: Mutex::new(VecDeque::new()),
                shutdown_calls: AtomicUsize::new(0),
                shutdown_results: Mutex::new(VecDeque::new()),
            }
        }

        fn queue_batch(&self, batch: RawEventBatch) {
            self.poll_batches.lock().expect("poll batches").push_back(batch);
        }

        fn queue_shutdown_result(&self, result: Result<Ack, SdkError>) {
            self.shutdown_results.lock().expect("shutdown results").push_back(result);
        }

        fn queue_send_result(&self, result: Result<crate::MessageId, SdkError>) {
            self.send_results.lock().expect("send results").push_back(result);
        }
    }

    impl SdkBackend for MockBackend {
        fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
            let runtime_id = format!("rt-{}", self.runtime_seq.fetch_add(1, Ordering::Relaxed));
            let mut effective_capabilities = crate::required_capabilities(req.profile)
                .iter()
                .map(|capability| (*capability).to_owned())
                .collect::<Vec<_>>();
            if !effective_capabilities
                .iter()
                .any(|capability| capability == "sdk.capability.async_events")
            {
                effective_capabilities.push("sdk.capability.async_events".to_owned());
            }
            Ok(NegotiationResponse {
                runtime_id,
                active_contract_version: 2,
                effective_capabilities,
                effective_limits: EffectiveLimits {
                    max_poll_events: 32,
                    max_event_bytes: 8_192,
                    max_batch_bytes: 65_536,
                    max_extension_keys: 32,
                    idempotency_ttl_ms: 60_000,
                },
                contract_release: "v2.5".to_owned(),
                schema_namespace: "v2".to_owned(),
            })
        }

        fn send(&self, _req: RawSendRequest) -> Result<crate::MessageId, SdkError> {
            self.send_results
                .lock()
                .expect("send results")
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(crate::MessageId(format!(
                        "msg-{}",
                        self.send_seq.fetch_add(1, Ordering::Relaxed)
                    )))
                })
        }

        fn cancel(&self, _id: crate::MessageId) -> Result<CancelResult, SdkError> {
            Ok(CancelResult::Accepted)
        }

        fn status(&self, id: crate::MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
            Ok(Some(DeliverySnapshot {
                message_id: id,
                state: RawDeliveryState::Sent,
                terminal: false,
                last_updated_ms: 10,
                attempts: 1,
                reason_code: None,
            }))
        }

        fn configure(
            &self,
            _expected_revision: u64,
            _patch: crate::ConfigPatch,
        ) -> Result<Ack, SdkError> {
            Ok(Ack { accepted: true, revision: Some(1) })
        }

        fn poll_events(
            &self,
            cursor: Option<EventCursor>,
            _max: usize,
        ) -> Result<RawEventBatch, SdkError> {
            self.poll_batches
                .lock()
                .expect("poll batches")
                .pop_front()
                .ok_or_else(|| {
                    SdkError::new(
                        code::RUNTIME_STREAM_DEGRADED,
                        SdkErrorCategory::Runtime,
                        "empty",
                    )
                        .with_retryable(false)
                })
                .or_else(|_| {
                    Ok(RawEventBatch::empty(
                        cursor.unwrap_or_else(|| EventCursor("cursor-0".to_owned())),
                    ))
                })
        }

        fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
            Ok(RuntimeSnapshot {
                runtime_id: "rt-live".to_owned(),
                state: RuntimeState::Running,
                active_contract_version: 2,
                event_stream_position: 7,
                config_revision: 1,
                queued_messages: 1,
                in_flight_messages: 2,
            })
        }

        fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
            self.shutdown_calls.fetch_add(1, Ordering::Relaxed);
            self.shutdown_results
                .lock()
                .expect("shutdown results")
                .pop_front()
                .unwrap_or(Ok(Ack { accepted: true, revision: None }))
        }
    }

    impl SdkBackendAsyncEvents for MockBackend {
        fn subscribe_events(
            &self,
            _start: crate::SubscriptionStart,
        ) -> Result<EventSubscription, SdkError> {
            Ok(EventSubscription {
                start: crate::SubscriptionStart::Head,
                cursor: Some(EventCursor("cursor-1".to_owned())),
            })
        }
    }

    fn runtime_started_event() -> SdkEvent {
        SdkEvent {
            event_id: "evt-1".to_owned(),
            runtime_id: "rt-live".to_owned(),
            stream_id: "stream".to_owned(),
            seq_no: 1,
            contract_version: 2,
            ts_ms: 10,
            event_type: "RuntimeStateChanged".to_owned(),
            severity: RawSeverity::Info,
            source_component: "test".to_owned(),
            operation_id: None,
            message_id: None,
            peer_id: None,
            correlation_id: None,
            trace_id: None,
            payload: json!({ "from": "starting", "to": "running" }),
            extensions: BTreeMap::new(),
        }
    }

    fn stream_gap_event() -> SdkEvent {
        SdkEvent {
            event_id: "evt-2".to_owned(),
            runtime_id: "rt-live".to_owned(),
            stream_id: "stream".to_owned(),
            seq_no: 2,
            contract_version: 2,
            ts_ms: 20,
            event_type: "StreamGap".to_owned(),
            severity: RawSeverity::Warn,
            source_component: "test".to_owned(),
            operation_id: None,
            message_id: None,
            peer_id: None,
            correlation_id: None,
            trace_id: None,
            payload: json!({ "expected_seq_no": 3, "observed_seq_no": 6, "dropped_count": 3 }),
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn config_presets_map_to_expected_profiles() {
        assert_eq!(Config::mobile_default().profile, Profile::MobileDefault);
        assert_eq!(Config::mobile_default().sdk_config.profile, CoreProfile::DesktopLocalRuntime);
        assert_eq!(Config::desktop_default().sdk_config.profile, CoreProfile::DesktopFull);
        assert_eq!(Config::embedded_default().sdk_config.profile, CoreProfile::EmbeddedAlloc);
    }

    #[test]
    fn client_restarts_by_recreating_inner_client() {
        let backend = MockBackend::new();
        let app = Client::new(backend);
        let first = app.start(Config::desktop_default()).expect("first start");
        app.stop(ShutdownMode::Immediate).expect("stop");
        let second = app.start(Config::desktop_default()).expect("second start");
        assert_ne!(first.runtime_id, second.runtime_id);
    }

    #[test]
    fn client_send_and_status_hide_raw_sdk_types() {
        let backend = MockBackend::new();
        let app = Client::new(backend);
        app.start(Config::desktop_default()).expect("start");
        let receipt = app
            .send(
                SendRequest::new("src", "dst", json!({ "body": "hello" }))
                    .with_correlation_id("corr-1"),
            )
            .expect("send");
        assert_eq!(receipt.profile, Profile::DesktopDefault);
        assert_eq!(receipt.correlation_id.as_deref(), Some("corr-1"));

        let status = app
            .delivery_status(receipt.message_id.as_str())
            .expect("delivery status")
            .expect("snapshot");
        assert_eq!(status.state, DeliveryState::Sent);
    }

    #[test]
    fn client_status_reports_degraded_after_gap_event() {
        let backend = MockBackend::new();
        backend.queue_batch(RawEventBatch {
            events: vec![runtime_started_event(), stream_gap_event()],
            next_cursor: EventCursor("cursor-2".to_owned()),
            dropped_count: 3,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        });

        let app = Client::new(backend);
        app.start(Config::desktop_default()).expect("start");
        let mut stream = app.subscribe_events(SubscriptionStart::Head).expect("subscribe");
        let batch = stream.next_batch().expect("next batch");
        assert_eq!(batch.events.len(), 2);

        let status = app.status().expect("status");
        assert_eq!(status.state, RunState::Degraded);
    }

    #[test]
    fn client_returns_not_started_before_start() {
        let app = Client::new(MockBackend::new());
        let err = app
            .send(SendRequest::new("src", "dst", json!({ "body": "hello" })))
            .expect_err("send should fail");
        assert_eq!(err.code.as_str(), "SDK_APP_RUNTIME_NOT_STARTED");
        assert!(!err.user_action_required);
    }

    #[test]
    fn failed_stop_preserves_live_session_state() {
        let backend = MockBackend::new();
        backend.queue_shutdown_result(Err(SdkError::new(
            code::INTERNAL,
            SdkErrorCategory::Internal,
            "shutdown failed",
        )));
        let app = Client::new(backend);
        app.start(Config::desktop_default()).expect("start");

        let err = app.stop(ShutdownMode::Immediate).expect_err("stop should fail");
        assert_eq!(err.code.as_str(), "SDK_APP_INTERNAL_UNEXPECTED_FAILURE");

        let receipt = app
            .send(SendRequest::new("src", "dst", json!({ "body": "still-live" })))
            .expect("send after failed stop");
        assert_eq!(receipt.profile, Profile::DesktopDefault);
    }

    #[test]
    fn restart_propagates_stop_failures() {
        let backend = MockBackend::new();
        backend.queue_shutdown_result(Err(SdkError::new(
            code::INTERNAL,
            SdkErrorCategory::Internal,
            "shutdown failed",
        )));
        let app = Client::new(backend);
        app.start(Config::desktop_default()).expect("start");

        let err = app
            .restart(Config::desktop_default())
            .expect_err("restart should fail when stop fails");
        assert_eq!(err.code.as_str(), "SDK_APP_INTERNAL_UNEXPECTED_FAILURE");
    }

    #[test]
    fn delivery_plan_tracks_profile_defaults() {
        let config = Config::desktop_default();
        let plan = config.delivery_plan();

        assert_eq!(plan.profile, Profile::DesktopDefault);
        assert_eq!(plan.retry.max_attempts, 5);
        assert!(plan.reconnect.enabled);
        assert_eq!(plan.default_event_batch_size, 64);
        assert!(plan.redaction_enabled);
    }

    #[test]
    fn send_with_profile_defaults_retries_queue_pressure() {
        let backend = MockBackend::new();
        backend.queue_send_result(Err(SdkError::new(
            "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
            SdkErrorCategory::Runtime,
            "full",
        )
        .with_retryable(true)));
        let app = Client::new(backend);
        app.start(Config::desktop_default()).expect("start");

        let report = app
            .send_with_profile_defaults(SendRequest::new("src", "dst", json!({ "body": "hello" })))
            .expect("report");

        assert_eq!(report.attempts.len(), 1);
        assert_eq!(report.attempts[0].disposition, super::super::delivery::AttemptDisposition::Retried);
        assert!(report.attempts[0].queue_pressure);
        assert_eq!(report.receipt.profile, Profile::DesktopDefault);
    }

    #[test]
    fn send_with_options_can_fail_fast_on_queue_pressure() {
        let backend = MockBackend::new();
        backend.queue_send_result(Err(SdkError::new(
            "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
            SdkErrorCategory::Runtime,
            "full",
        )
        .with_retryable(true)));
        let app = Client::new(backend);
        app.start(Config::desktop_default()).expect("start");

        let err = app
            .send_with_options(
                SendRequest::new("src", "dst", json!({ "body": "hello" })),
                super::super::delivery::DeliveryOptions {
                    queue_pressure_strategy: Some(super::super::delivery::QueuePressureStrategy::FailFast),
                    ..Default::default()
                },
            )
            .expect_err("queue pressure should fail fast");

        assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_QUEUE_PRESSURE");
    }

    #[test]
    fn send_with_options_maps_retry_exhaustion() {
        let backend = MockBackend::new();
        backend.queue_send_result(Err(
            SdkError::new(code::INTERNAL, SdkErrorCategory::Internal, "temporary")
                .with_retryable(true),
        ));
        backend.queue_send_result(Err(
            SdkError::new(code::INTERNAL, SdkErrorCategory::Internal, "temporary")
                .with_retryable(true),
        ));
        let app = Client::new(backend);
        app.start(Config::testing_default()).expect("start");

        let err = app
            .send_with_options(
                SendRequest::new("src", "dst", json!({ "body": "hello" })),
                DeliveryOptions { max_attempts: Some(2), ..Default::default() },
            )
            .expect_err("retry exhaustion");

        assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_RETRY_EXHAUSTED");
        assert_eq!(err.cause_code.as_deref(), Some("SDK_INTERNAL_ERROR"));
    }
}
