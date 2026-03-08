use crate::easy::capabilities::EasyCapabilitySummary;
use crate::easy::errors::EasyError;
#[cfg(feature = "sdk-async")]
use crate::easy::events::{
    map_event_batch, subscription_cursor, EasyEventBatch, EasySubscriptionStart,
};
use crate::{
    Client, ClientHandle, DeliverySnapshot, DeliveryState, EventCursor, LxmfSdk, Profile,
    RuntimeSnapshot, RuntimeState, SdkBackend, SdkConfig, SendRequest, ShutdownMode, StartRequest,
};
#[cfg(feature = "sdk-async")]
use crate::{LxmfSdkAsync, SdkBackendAsyncEvents};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EasyProfile {
    MobileDefault,
    DesktopDefault,
    EmbeddedDefault,
    TestingDefault,
}

impl EasyProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MobileDefault => "mobile_default",
            Self::DesktopDefault => "desktop_default",
            Self::EmbeddedDefault => "embedded_default",
            Self::TestingDefault => "testing_default",
        }
    }

    pub fn to_sdk_profile(&self) -> Profile {
        match self {
            Self::MobileDefault => Profile::DesktopLocalRuntime,
            Self::DesktopDefault => Profile::DesktopFull,
            Self::EmbeddedDefault => Profile::EmbeddedAlloc,
            Self::TestingDefault => Profile::DesktopLocalRuntime,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EasyConfig {
    pub profile: EasyProfile,
    pub sdk_config: SdkConfig,
    pub supported_contract_versions: Vec<u16>,
    pub requested_capabilities: Vec<String>,
    pub event_batch_size: Option<usize>,
}

impl EasyConfig {
    pub fn mobile_default() -> Self {
        Self {
            profile: EasyProfile::MobileDefault,
            sdk_config: SdkConfig::desktop_local_default(),
            supported_contract_versions: vec![2],
            requested_capabilities: Vec::new(),
            event_batch_size: Some(32),
        }
    }

    pub fn desktop_default() -> Self {
        Self {
            profile: EasyProfile::DesktopDefault,
            sdk_config: SdkConfig::desktop_full_default(),
            supported_contract_versions: vec![2],
            requested_capabilities: Vec::new(),
            event_batch_size: Some(64),
        }
    }

    pub fn embedded_default() -> Self {
        Self {
            profile: EasyProfile::EmbeddedDefault,
            sdk_config: SdkConfig::embedded_alloc_default(),
            supported_contract_versions: vec![2],
            requested_capabilities: Vec::new(),
            event_batch_size: Some(16),
        }
    }

    pub fn testing_default() -> Self {
        let mut sdk_config = SdkConfig::desktop_local_default();
        sdk_config.event_stream.max_poll_events = 32;
        Self {
            profile: EasyProfile::TestingDefault,
            sdk_config,
            supported_contract_versions: vec![2],
            requested_capabilities: Vec::new(),
            event_batch_size: Some(16),
        }
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
pub enum EasyRunState {
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
pub struct EasyHandle {
    pub runtime_id: String,
    pub profile: EasyProfile,
    pub capabilities: EasyCapabilitySummary,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EasyRuntimeStatus {
    pub runtime_id: Option<String>,
    pub state: EasyRunState,
    pub profile: Option<EasyProfile>,
    pub capabilities: Option<EasyCapabilitySummary>,
    pub queued_messages: u64,
    pub in_flight_messages: u64,
    pub event_stream_position: u64,
    pub config_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EasySendRequest {
    pub source: String,
    pub destination: String,
    pub payload: JsonValue,
    pub idempotency_key: Option<String>,
    pub ttl_ms: Option<u64>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl EasySendRequest {
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

    fn into_raw(self) -> SendRequest {
        SendRequest {
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
pub struct EasySendReceipt {
    pub runtime_id: String,
    pub message_id: String,
    pub profile: EasyProfile,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EasyDeliveryState {
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
pub struct EasyDeliveryStatus {
    pub message_id: String,
    pub state: EasyDeliveryState,
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

    fn send(&self, req: SendRequest) -> Result<crate::MessageId, crate::SdkError> {
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

struct EasySessionState {
    handle: ClientHandle,
    config: EasyConfig,
    degraded: bool,
}

struct EasyState<B: SdkBackend> {
    client: Option<Arc<Client<SharedBackend<B>>>>,
    session: Option<Arc<Mutex<EasySessionState>>>,
    lifecycle: EasyRunState,
}

impl<B: SdkBackend> Default for EasyState<B> {
    fn default() -> Self {
        Self { client: None, session: None, lifecycle: EasyRunState::New }
    }
}

pub struct EasyClient<B: SdkBackend> {
    backend: Arc<B>,
    state: Mutex<EasyState<B>>,
}

impl<B: SdkBackend> EasyClient<B> {
    pub fn new(backend: B) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    pub fn from_arc(backend: Arc<B>) -> Self {
        Self { backend, state: Mutex::new(EasyState::default()) }
    }

    pub fn start(&self, config: EasyConfig) -> Result<EasyHandle, EasyError> {
        let mut state = self.state.lock().expect("easy client mutex poisoned");
        if state.client.is_none() && state.session.is_some() {
            state.session = None;
            state.lifecycle = EasyRunState::Stopped;
        }
        if let Some(session) = state.session.as_ref() {
            let session = session.lock().expect("easy session mutex poisoned");
            return if session.config == config {
                Ok(EasyHandle {
                    runtime_id: session.handle.runtime_id.clone(),
                    profile: session.config.profile.clone(),
                    capabilities: EasyCapabilitySummary::from(&session.handle),
                })
            } else {
                Err(EasyError::already_running_different_config())
            };
        }

        state.lifecycle = EasyRunState::Starting;
        let client = Arc::new(Client::new(SharedBackend(Arc::clone(&self.backend))));
        let handle = match client.start(config.start_request()) {
            Ok(handle) => handle,
            Err(err) => {
                state.lifecycle = EasyRunState::New;
                return Err(EasyError::from(err));
            }
        };
        let session = Arc::new(Mutex::new(EasySessionState {
            handle: handle.clone(),
            config: config.clone(),
            degraded: false,
        }));
        state.client = Some(Arc::clone(&client));
        state.session = Some(session);
        state.lifecycle = EasyRunState::Running;

        Ok(EasyHandle {
            runtime_id: handle.runtime_id.clone(),
            profile: config.profile,
            capabilities: EasyCapabilitySummary::from(&handle),
        })
    }

    pub fn restart(&self, config: EasyConfig) -> Result<EasyHandle, EasyError> {
        self.stop(ShutdownMode::Immediate)?;
        self.start(config)
    }

    pub fn send(&self, request: EasySendRequest) -> Result<EasySendReceipt, EasyError> {
        let state = self.state.lock().expect("easy client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(EasyError::not_started());
        };
        let Some(session) = state.session.as_ref() else {
            return Err(EasyError::not_started());
        };
        let session = session.lock().expect("easy session mutex poisoned");
        let correlation_id = request.correlation_id.clone();
        let message_id = client.send(request.into_raw()).map_err(EasyError::from)?;
        Ok(EasySendReceipt {
            runtime_id: session.handle.runtime_id.clone(),
            message_id: message_id.0,
            profile: session.config.profile.clone(),
            correlation_id,
        })
    }

    pub fn delivery_status(
        &self,
        message_id: impl Into<crate::MessageId>,
    ) -> Result<Option<EasyDeliveryStatus>, EasyError> {
        let state = self.state.lock().expect("easy client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(EasyError::not_started());
        };
        let snapshot = client.status(message_id.into()).map_err(EasyError::from)?;
        Ok(snapshot.map(map_delivery_snapshot))
    }

    pub fn status(&self) -> Result<EasyRuntimeStatus, EasyError> {
        let state = self.state.lock().expect("easy client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Ok(EasyRuntimeStatus {
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
            return Err(EasyError::not_started());
        };
        let snapshot = client.snapshot().map_err(EasyError::from)?;
        let session = session.lock().expect("easy session mutex poisoned");
        Ok(EasyRuntimeStatus {
            runtime_id: Some(snapshot.runtime_id.clone()),
            state: map_runtime_state(snapshot.state, session.degraded),
            profile: Some(session.config.profile.clone()),
            capabilities: Some(EasyCapabilitySummary::from(&session.handle)),
            queued_messages: snapshot.queued_messages,
            in_flight_messages: snapshot.in_flight_messages,
            event_stream_position: snapshot.event_stream_position,
            config_revision: snapshot.config_revision,
        })
    }

    pub fn stop(&self, mode: ShutdownMode) -> Result<(), EasyError> {
        let mut state = self.state.lock().expect("easy client mutex poisoned");
        let Some(client) = state.client.as_ref().cloned() else {
            state.session = None;
            state.lifecycle = EasyRunState::Stopped;
            return Ok(());
        };
        let previous_lifecycle = state.lifecycle.clone();
        state.lifecycle = EasyRunState::Stopping;
        if let Err(err) = client.shutdown(mode) {
            state.lifecycle = previous_lifecycle;
            return Err(EasyError::from(err));
        }
        state.client = None;
        state.session = None;
        state.lifecycle = EasyRunState::Stopped;
        Ok(())
    }

    #[cfg(feature = "sdk-async")]
    pub fn subscribe_events(
        &self,
        start: EasySubscriptionStart,
    ) -> Result<EasyEventStream<B>, EasyError>
    where
        B: SdkBackendAsyncEvents,
    {
        let state = self.state.lock().expect("easy client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(EasyError::not_started());
        };
        let Some(session) = state.session.as_ref() else {
            return Err(EasyError::not_started());
        };
        let subscription =
            client.subscribe_events(start.clone().into()).map_err(EasyError::from)?;
        let session_guard = session.lock().expect("easy session mutex poisoned");
        let max_batch_size = session_guard
            .config
            .event_batch_size
            .unwrap_or(session_guard.handle.effective_limits.max_poll_events)
            .min(session_guard.handle.effective_limits.max_poll_events)
            .max(1);
        let profile = session_guard.config.profile.clone();
        drop(session_guard);
        Ok(EasyEventStream {
            client: Arc::clone(client),
            session: Arc::clone(session),
            cursor: subscription_cursor(&subscription),
            max_batch_size,
            profile,
        })
    }
}

#[cfg(feature = "rpc-backend")]
impl EasyClient<crate::RpcBackendClient> {
    pub fn rpc(endpoint: impl Into<String>) -> Self {
        Self::new(crate::RpcBackendClient::new(endpoint.into()))
    }
}

#[cfg(feature = "sdk-async")]
pub struct EasyEventStream<B: SdkBackendAsyncEvents> {
    client: Arc<Client<SharedBackend<B>>>,
    session: Arc<Mutex<EasySessionState>>,
    cursor: Option<EventCursor>,
    max_batch_size: usize,
    profile: EasyProfile,
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> EasyEventStream<B> {
    pub fn next_batch(&mut self) -> Result<EasyEventBatch, EasyError> {
        let batch = self
            .client
            .poll_events(self.cursor.clone(), self.max_batch_size)
            .map_err(EasyError::from)?;
        self.cursor = Some(batch.next_cursor.clone());

        let easy_batch = map_event_batch(batch, self.profile.as_str());
        if easy_batch.dropped_count > 0
            || easy_batch.events.iter().any(|event| {
                matches!(event.kind, crate::easy::events::EasyEventKind::StreamGapDetected(_))
            })
        {
            let mut session = self.session.lock().expect("easy session mutex poisoned");
            session.degraded = true;
        }
        Ok(easy_batch)
    }

    pub fn reset(&mut self) {
        self.cursor = None;
        let mut session = self.session.lock().expect("easy session mutex poisoned");
        session.degraded = false;
    }
}

fn map_runtime_state(state: RuntimeState, degraded: bool) -> EasyRunState {
    if degraded && state == RuntimeState::Running {
        return EasyRunState::Degraded;
    }
    match state {
        RuntimeState::New => EasyRunState::New,
        RuntimeState::Starting => EasyRunState::Starting,
        RuntimeState::Running => EasyRunState::Running,
        RuntimeState::Draining => EasyRunState::Stopping,
        RuntimeState::Stopped => EasyRunState::Stopped,
        RuntimeState::Failed | RuntimeState::Unknown => EasyRunState::Failed,
    }
}

fn map_delivery_snapshot(snapshot: DeliverySnapshot) -> EasyDeliveryStatus {
    let state = match snapshot.state {
        DeliveryState::Queued => EasyDeliveryState::Queued,
        DeliveryState::Dispatching | DeliveryState::InFlight => EasyDeliveryState::Dispatching,
        DeliveryState::Sent => EasyDeliveryState::Sent,
        DeliveryState::Delivered => EasyDeliveryState::Delivered,
        DeliveryState::Failed => EasyDeliveryState::Failed,
        DeliveryState::Cancelled => EasyDeliveryState::Cancelled,
        DeliveryState::Expired => EasyDeliveryState::Expired,
        DeliveryState::Rejected => EasyDeliveryState::Rejected,
        DeliveryState::Unknown => EasyDeliveryState::Unknown,
    };
    EasyDeliveryStatus {
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
        EasyClient, EasyConfig, EasyDeliveryState, EasyProfile, EasyRunState, EasySendRequest,
        EasySubscriptionStart,
    };
    use crate::error::{code, ErrorCategory, SdkError};
    use crate::event::{EventBatch, EventCursor, EventSubscription, SdkEvent, Severity};
    use crate::{
        Ack, CancelResult, DeliverySnapshot, DeliveryState, EffectiveLimits, NegotiationRequest,
        NegotiationResponse, Profile, RuntimeSnapshot, RuntimeState, SdkBackend,
        SdkBackendAsyncEvents, SendRequest, ShutdownMode,
    };
    use serde_json::json;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockBackend {
        runtime_seq: AtomicUsize,
        send_seq: AtomicUsize,
        poll_batches: Mutex<VecDeque<EventBatch>>,
        shutdown_calls: AtomicUsize,
        shutdown_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                runtime_seq: AtomicUsize::new(1),
                send_seq: AtomicUsize::new(1),
                poll_batches: Mutex::new(VecDeque::new()),
                shutdown_calls: AtomicUsize::new(0),
                shutdown_results: Mutex::new(VecDeque::new()),
            }
        }

        fn queue_batch(&self, batch: EventBatch) {
            self.poll_batches.lock().expect("poll batches").push_back(batch);
        }

        fn queue_shutdown_result(&self, result: Result<Ack, SdkError>) {
            self.shutdown_results.lock().expect("shutdown results").push_back(result);
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

        fn send(&self, _req: SendRequest) -> Result<crate::MessageId, SdkError> {
            Ok(crate::MessageId(format!("msg-{}", self.send_seq.fetch_add(1, Ordering::Relaxed))))
        }

        fn cancel(&self, _id: crate::MessageId) -> Result<CancelResult, SdkError> {
            Ok(CancelResult::Accepted)
        }

        fn status(&self, id: crate::MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
            Ok(Some(DeliverySnapshot {
                message_id: id,
                state: DeliveryState::Sent,
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
        ) -> Result<EventBatch, SdkError> {
            self.poll_batches
                .lock()
                .expect("poll batches")
                .pop_front()
                .ok_or_else(|| {
                    SdkError::new(code::RUNTIME_STREAM_DEGRADED, ErrorCategory::Runtime, "empty")
                        .with_retryable(false)
                })
                .or_else(|_| {
                    Ok(EventBatch::empty(
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
            severity: Severity::Info,
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
            severity: Severity::Warn,
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
    fn easy_config_presets_map_to_expected_profiles() {
        assert_eq!(EasyConfig::mobile_default().profile, EasyProfile::MobileDefault);
        assert_eq!(EasyConfig::mobile_default().sdk_config.profile, Profile::DesktopLocalRuntime);
        assert_eq!(EasyConfig::desktop_default().sdk_config.profile, Profile::DesktopFull);
        assert_eq!(EasyConfig::embedded_default().sdk_config.profile, Profile::EmbeddedAlloc);
    }

    #[test]
    fn easy_client_restarts_by_recreating_inner_client() {
        let backend = MockBackend::new();
        let easy = EasyClient::new(backend);
        let first = easy.start(EasyConfig::desktop_default()).expect("first start");
        easy.stop(ShutdownMode::Immediate).expect("stop");
        let second = easy.start(EasyConfig::desktop_default()).expect("second start");
        assert_ne!(first.runtime_id, second.runtime_id);
    }

    #[test]
    fn easy_client_send_and_status_hide_raw_sdk_types() {
        let backend = MockBackend::new();
        let easy = EasyClient::new(backend);
        easy.start(EasyConfig::desktop_default()).expect("start");
        let receipt = easy
            .send(
                EasySendRequest::new("src", "dst", json!({ "body": "hello" }))
                    .with_correlation_id("corr-1"),
            )
            .expect("send");
        assert_eq!(receipt.profile, EasyProfile::DesktopDefault);
        assert_eq!(receipt.correlation_id.as_deref(), Some("corr-1"));

        let status = easy
            .delivery_status(receipt.message_id.as_str())
            .expect("delivery status")
            .expect("snapshot");
        assert_eq!(status.state, EasyDeliveryState::Sent);
    }

    #[test]
    fn easy_client_status_reports_degraded_after_gap_event() {
        let backend = MockBackend::new();
        backend.queue_batch(EventBatch {
            events: vec![runtime_started_event(), stream_gap_event()],
            next_cursor: EventCursor("cursor-2".to_owned()),
            dropped_count: 3,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        });

        let easy = EasyClient::new(backend);
        easy.start(EasyConfig::desktop_default()).expect("start");
        let mut stream = easy.subscribe_events(EasySubscriptionStart::Head).expect("subscribe");
        let batch = stream.next_batch().expect("next batch");
        assert_eq!(batch.events.len(), 2);

        let status = easy.status().expect("status");
        assert_eq!(status.state, EasyRunState::Degraded);
    }

    #[test]
    fn easy_client_returns_not_started_before_start() {
        let easy = EasyClient::new(MockBackend::new());
        let err = easy
            .send(EasySendRequest::new("src", "dst", json!({ "body": "hello" })))
            .expect_err("send should fail");
        assert_eq!(err.code.as_str(), "EASY_RUNTIME_NOT_STARTED");
        assert!(!err.user_action_required);
    }

    #[test]
    fn failed_stop_preserves_live_session_state() {
        let backend = MockBackend::new();
        backend.queue_shutdown_result(Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Internal,
            "shutdown failed",
        )));
        let easy = EasyClient::new(backend);
        easy.start(EasyConfig::desktop_default()).expect("start");

        let err = easy.stop(ShutdownMode::Immediate).expect_err("stop should fail");
        assert_eq!(err.code.as_str(), "EASY_INTERNAL_UNEXPECTED_FAILURE");

        let receipt = easy
            .send(EasySendRequest::new("src", "dst", json!({ "body": "still-live" })))
            .expect("send after failed stop");
        assert_eq!(receipt.profile, EasyProfile::DesktopDefault);
    }

    #[test]
    fn restart_propagates_stop_failures() {
        let backend = MockBackend::new();
        backend.queue_shutdown_result(Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Internal,
            "shutdown failed",
        )));
        let easy = EasyClient::new(backend);
        easy.start(EasyConfig::desktop_default()).expect("start");

        let err = easy
            .restart(EasyConfig::desktop_default())
            .expect_err("restart should fail when stop fails");
        assert_eq!(err.code.as_str(), "EASY_INTERNAL_UNEXPECTED_FAILURE");
    }
}
