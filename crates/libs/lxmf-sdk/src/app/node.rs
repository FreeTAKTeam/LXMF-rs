use super::discovery::{
    BootstrapRequest, Contact, ContactPage, ContactUpdate, Identity, PeerDirectoryEntry, Presence,
    PresencePage,
};
use super::capabilities::CapabilitySummary;
use super::delivery::{
    AttemptDecision, AttemptDisposition, DeliveryAttempt, DeliveryOptions, DeliveryPlan, SendReport,
};
use super::envelope::{Envelope, EnvelopeKind, EnvelopeResponse};
use super::errors::Error;
#[cfg(feature = "sdk-async")]
use super::events::{map_event_batch, subscription_cursor, EventBatch, SubscriptionStart};
use super::operations::{OperationEntry, OperationRegistry, RegistryError};
use crate::{
    Client as CoreClient, ClientHandle, DeliverySnapshot, DeliveryState as RawDeliveryState,
    EventCursor, LxmfSdk, Profile as CoreProfile, RuntimeSnapshot, RuntimeState, SdkBackend,
    SdkConfig, SendRequest as RawSendRequest, ShutdownMode, StartRequest,
};
use crate::domain::{
    ContactListRequest, ContactUpdateRequest, IdentityBootstrapRequest, PresenceListRequest,
    RemoteCommandRequest,
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
    #[serde(default)]
    pub custom_operations: Vec<OperationEntry>,
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

    pub fn operation_registry(&self) -> Result<OperationRegistry, RegistryError> {
        OperationRegistry::built_in().merged(self.custom_operations.clone())
    }

    pub fn with_custom_operation(mut self, operation: OperationEntry) -> Self {
        self.custom_operations.push(operation);
        self
    }

    pub fn with_custom_operations(
        mut self,
        operations: impl IntoIterator<Item = OperationEntry>,
    ) -> Self {
        self.custom_operations.extend(operations);
        self
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

    pub fn operation_registry(&self) -> Result<OperationRegistry, Error> {
        match self.active_config() {
            Ok(config) => config.operation_registry().map_err(Error::from),
            Err(err) if matches!(err.code, super::errors::ErrorCode::RuntimeNotStarted) => {
                Ok(OperationRegistry::built_in().clone())
            }
            Err(err) => Err(err),
        }
    }

    pub fn query(
        &self,
        operation_id: impl Into<super::operations::OperationId>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, Error> {
        self.execute_envelope(Envelope::query(operation_id, payload))
    }

    pub fn command(
        &self,
        operation_id: impl Into<super::operations::OperationId>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, Error> {
        self.execute_envelope(Envelope::command(operation_id, payload))
    }

    pub fn execute_envelope(&self, envelope: Envelope) -> Result<EnvelopeResponse, Error> {
        let registry = self.operation_registry()?;
        let envelope = envelope.normalized(&registry).map_err(|err| match err {
            super::envelope::EnvelopeValidationError::UnknownOperation { operation_id } => {
                invalid_envelope("unknown operation id", operation_id.as_str())
            }
            super::envelope::EnvelopeValidationError::KindMismatch { operation_id, .. } => {
                invalid_envelope(
                    "envelope kind does not match registered operation kind",
                    operation_id.as_str(),
                )
            }
        })?;
        let entry = registry
            .get(envelope.operation_id.as_str())
            .cloned()
            .expect("normalized envelope should resolve to a registered operation");
        let canonical_id = entry.id.clone();

        let Envelope {
            operation_id: _,
            kind: _,
            target,
            correlation_id,
            timeout_ms,
            payload,
            extensions,
        } = envelope;

        match canonical_id.as_str() {
            "app.runtime.start" => {
                let config: Config = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid runtime start payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let handle = self.start(config)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(handle).expect("runtime handle should serialize"),
                ))
            }
            "app.runtime.restart" => {
                let config: Config = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid runtime restart payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let handle = self.restart(config)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(handle).expect("runtime handle should serialize"),
                ))
            }
            "app.runtime.stop" => {
                let mode = payload
                    .get("mode")
                    .cloned()
                    .map(serde_json::from_value::<ShutdownMode>)
                    .transpose()
                    .map_err(|err| {
                        invalid_envelope(
                            format!("invalid runtime stop payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?
                    .unwrap_or(ShutdownMode::Graceful);
                self.stop(mode.clone())?;
                Ok(envelope_result(canonical_id, correlation_id, serde_json::json!({
                    "accepted": true,
                    "mode": mode,
                })))
            }
            "app.runtime.status" => Ok(envelope_result(
                canonical_id,
                correlation_id,
                serde_json::to_value(self.status()?).expect("runtime status should serialize"),
            )),
            "app.delivery.send" => {
                let request: SendRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid delivery send payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let receipt = self.send(request)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(receipt).expect("send receipt should serialize"),
                ))
            }
            "app.delivery.status" => {
                let message_id =
                    payload
                        .get("message_id")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            invalid_envelope(
                                "delivery status envelope requires payload.message_id",
                                canonical_id.as_str(),
                            )
                        })?;
                let status = self.delivery_status(message_id.to_owned())?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(status).expect("delivery status should serialize"),
                ))
            }
            "app.event.poll" => {
                let cursor = payload
                    .get("cursor")
                    .and_then(|value| value.as_str())
                    .map(|value| EventCursor(value.to_owned()));
                let max = payload
                    .get("max")
                    .and_then(|value| value.as_u64())
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(32)
                    .max(1);
                let batch = self.backend.poll_events(cursor, max).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(batch).expect("event batch should serialize"),
                ))
            }
            "app.identity.list" => Ok(envelope_result(
                canonical_id,
                correlation_id,
                serde_json::to_value(self.backend.identity_list().map_err(Error::from)?)
                    .expect("identity list should serialize"),
            )),
            "app.identity.announce" => {
                self.backend.identity_announce_now().map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::json!({ "accepted": true }),
                ))
            }
            "app.identity.presence.list" => {
                let req: PresenceListRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid presence list payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.identity_presence_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("presence list should serialize"),
                ))
            }
            "app.contact.list" => {
                let req: ContactListRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid contact list payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.identity_contact_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("contact list should serialize"),
                ))
            }
            "app.contact.update" => {
                let req: ContactUpdateRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid contact update payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.identity_contact_update(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("contact update should serialize"),
                ))
            }
            "app.identity.bootstrap" => {
                let req: IdentityBootstrapRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid bootstrap payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.identity_bootstrap(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("bootstrap should serialize"),
                ))
            }
            _ if matches!(entry.kind, super::operations::OperationKind::Query) => self
                .backend
                .envelope_execute(Envelope {
                    operation_id: canonical_id,
                    kind: EnvelopeKind::Query,
                    target,
                    correlation_id,
                    timeout_ms,
                    payload,
                    extensions,
                })
                .map_err(Error::from),
            _ => {
                let response = self
                    .backend
                    .command_invoke(RemoteCommandRequest {
                        command: canonical_id.as_str().to_owned(),
                        target,
                        payload,
                        timeout_ms,
                        extensions,
                    })
                    .map_err(Error::from)?;
                Ok(EnvelopeResponse {
                    operation_id: canonical_id,
                    kind: EnvelopeKind::Result,
                    accepted: response.accepted,
                    correlation_id,
                    payload: response.payload,
                    extensions: response.extensions,
                })
            }
        }
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
                    let total_delay_ms = attempts
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

    pub fn identities(&self) -> Result<Vec<Identity>, Error> {
        let identities = self.backend.identity_list().map_err(Error::from)?;
        Ok(identities.into_iter().map(Identity::from).collect())
    }

    pub fn announce_now(&self) -> Result<(), Error> {
        self.backend.identity_announce_now().map_err(Error::from)?;
        Ok(())
    }

    pub fn contacts(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<ContactPage, Error> {
        let result = self
            .backend
            .identity_contact_list(ContactListRequest {
                cursor,
                limit,
                extensions: BTreeMap::new(),
            })
            .map_err(Error::from)?;
        Ok(ContactPage::from(result))
    }

    pub fn presence(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<PresencePage, Error> {
        let result = self
            .backend
            .identity_presence_list(PresenceListRequest {
                cursor,
                limit,
                extensions: BTreeMap::new(),
            })
            .map_err(Error::from)?;
        Ok(PresencePage::from(result))
    }

    pub fn update_contact(&self, update: ContactUpdate) -> Result<Contact, Error> {
        let contact = self.backend.identity_contact_update(update.into()).map_err(Error::from)?;
        Ok(Contact::from(contact))
    }

    pub fn bootstrap_identity(&self, request: BootstrapRequest) -> Result<Contact, Error> {
        let contact = self.backend.identity_bootstrap(request.into()).map_err(Error::from)?;
        Ok(Contact::from(contact))
    }

    pub fn peer_directory(&self, limit: Option<usize>) -> Result<Vec<PeerDirectoryEntry>, Error> {
        let mut entries = BTreeMap::<String, PeerDirectoryEntry>::new();

        for contact in self.collect_contacts(limit)? {
            entries.insert(
                contact.identity.clone(),
                PeerDirectoryEntry {
                    peer_id: contact.identity.clone(),
                    display_name: contact.display_name.clone(),
                    name_source: Some("contact".to_owned()),
                    trust_level: Some(contact.trust_level.clone()),
                    bootstrap: contact.bootstrap,
                    online: false,
                    last_seen_ts_ms: None,
                    first_seen_ts_ms: None,
                    seen_count: 0,
                    metadata: contact.metadata.clone(),
                    extensions: contact.extensions.clone(),
                },
            );
        }

        for presence in self.collect_presence(limit)? {
            let entry = entries.entry(presence.peer_id.clone()).or_insert_with(|| PeerDirectoryEntry {
                peer_id: presence.peer_id.clone(),
                display_name: presence.display_name.clone(),
                name_source: presence.name_source.clone(),
                trust_level: presence.trust_level.clone(),
                bootstrap: presence.bootstrap.unwrap_or(false),
                online: true,
                last_seen_ts_ms: Some(presence.last_seen_ts_ms),
                first_seen_ts_ms: Some(presence.first_seen_ts_ms),
                seen_count: presence.seen_count,
                metadata: BTreeMap::new(),
                extensions: presence.extensions.clone(),
            });
            entry.online = true;
            entry.last_seen_ts_ms = Some(presence.last_seen_ts_ms);
            entry.first_seen_ts_ms = Some(presence.first_seen_ts_ms);
            entry.seen_count = presence.seen_count;
            if entry.display_name.is_none() {
                entry.display_name = presence.display_name.clone();
            }
            if entry.name_source.is_none() {
                entry.name_source = presence.name_source.clone();
            }
            if entry.trust_level.is_none() {
                entry.trust_level = presence.trust_level.clone();
            }
            if !entry.bootstrap {
                entry.bootstrap = presence.bootstrap.unwrap_or(false);
            }
            for (key, value) in presence.extensions {
                entry.extensions.entry(key).or_insert(value);
            }
        }

        let mut values = entries.into_values().collect::<Vec<_>>();
        if let Some(limit) = limit {
            values.truncate(limit);
        }
        Ok(values)
    }

    fn collect_contacts(&self, limit: Option<usize>) -> Result<Vec<Contact>, Error> {
        let mut contacts = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.contacts(cursor.clone(), limit)?;
            contacts.extend(page.contacts);
            if let Some(limit) = limit {
                if contacts.len() >= limit {
                    contacts.truncate(limit);
                    break;
                }
            }
            match page.next_cursor {
                Some(next_cursor) if cursor.as_deref() != Some(next_cursor.as_str()) => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        Ok(contacts)
    }

    fn collect_presence(&self, limit: Option<usize>) -> Result<Vec<Presence>, Error> {
        let mut peers = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.presence(cursor.clone(), limit)?;
            peers.extend(page.peers);
            if let Some(limit) = limit {
                if peers.len() >= limit {
                    peers.truncate(limit);
                    break;
                }
            }
            match page.next_cursor {
                Some(next_cursor) if cursor.as_deref() != Some(next_cursor.as_str()) => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        Ok(peers)
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
    pub fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventStream<B>, Error>
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
        let subscription = client.subscribe_events(start.clone().into()).map_err(Error::from)?;
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
            || batch
                .events
                .iter()
                .any(|event| matches!(event.kind, super::events::EventKind::StreamGapDetected(_)))
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

fn invalid_envelope(message: impl Into<String>, operation_id: impl Into<String>) -> Error {
    let mut details = BTreeMap::new();
    details.insert(
        "operation_id".to_owned(),
        JsonValue::String(operation_id.into()),
    );
    Error {
        code: super::errors::ErrorCode::ValidationInvalidArgument,
        category: super::errors::ErrorCategory::Validation,
        retryable: false,
        terminal: true,
        user_action_required: true,
        message: message.into(),
        details,
        cause_code: None,
    }
}

fn envelope_result(
    operation_id: super::operations::OperationId,
    correlation_id: Option<String>,
    payload: JsonValue,
) -> EnvelopeResponse {
    EnvelopeResponse {
        operation_id,
        kind: EnvelopeKind::Result,
        accepted: true,
        correlation_id,
        payload,
        extensions: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Client, Config, DeliveryState, Envelope, EnvelopeKind, Profile, RunState, SendRequest,
        SubscriptionStart,
    };
    use crate::app::DeliveryOptions;
    use crate::app::{OperationEntry, OperationKind, TransportVariant};
    use crate::domain::TrustLevel;
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
        paginate_discovery: bool,
        poll_batches: Mutex<VecDeque<RawEventBatch>>,
        send_results: Mutex<VecDeque<Result<crate::MessageId, SdkError>>>,
        shutdown_calls: AtomicUsize,
        shutdown_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
        remote_command_results:
            Mutex<VecDeque<Result<crate::domain::RemoteCommandResponse, SdkError>>>,
        envelope_results:
            Mutex<VecDeque<Result<crate::app::EnvelopeResponse, SdkError>>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                runtime_seq: AtomicUsize::new(1),
                send_seq: AtomicUsize::new(1),
                paginate_discovery: false,
                poll_batches: Mutex::new(VecDeque::new()),
                send_results: Mutex::new(VecDeque::new()),
                shutdown_calls: AtomicUsize::new(0),
                shutdown_results: Mutex::new(VecDeque::new()),
                remote_command_results: Mutex::new(VecDeque::new()),
                envelope_results: Mutex::new(VecDeque::new()),
            }
        }

        fn new_paginated() -> Self {
            Self { paginate_discovery: true, ..Self::new() }
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

        fn queue_remote_command_result(
            &self,
            result: Result<crate::domain::RemoteCommandResponse, SdkError>,
        ) {
            self.remote_command_results
                .lock()
                .expect("remote command results")
                .push_back(result);
        }

        fn queue_envelope_result(
            &self,
            result: Result<crate::app::EnvelopeResponse, SdkError>,
        ) {
            self.envelope_results.lock().expect("envelope results").push_back(result);
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
            for capability in [
                "sdk.capability.identity_multi",
                "sdk.capability.identity_discovery",
                "sdk.capability.contact_management",
            ] {
                if !effective_capabilities.iter().any(|current| current == capability) {
                    effective_capabilities.push(capability.to_owned());
                }
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
            self.send_results.lock().expect("send results").pop_front().unwrap_or_else(|| {
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
                    SdkError::new(code::RUNTIME_STREAM_DEGRADED, SdkErrorCategory::Runtime, "empty")
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

        fn identity_list(&self) -> Result<Vec<crate::domain::IdentityBundle>, SdkError> {
            Ok(vec![crate::domain::IdentityBundle {
                identity: crate::domain::IdentityRef("alice".to_owned()),
                public_key: "pubkey".to_owned(),
                display_name: Some("Alice".to_owned()),
                capabilities: vec!["chat".to_owned()],
                extensions: BTreeMap::new(),
            }])
        }

        fn identity_contact_list(
            &self,
            req: crate::domain::ContactListRequest,
        ) -> Result<crate::domain::ContactListResult, SdkError> {
            let make_contact = |identity: &str, display_name: &str, trust_level, bootstrap| {
                crate::domain::ContactRecord {
                    identity: crate::domain::IdentityRef(identity.to_owned()),
                    display_name: Some(display_name.to_owned()),
                    trust_level,
                    bootstrap,
                    updated_ts_ms: 100,
                    metadata: BTreeMap::new(),
                    extensions: BTreeMap::from([(
                        "cursor".to_owned(),
                        serde_json::json!(req.cursor),
                    )]),
                }
            };
            if self.paginate_discovery {
                return Ok(match req.cursor.as_deref() {
                    None => crate::domain::ContactListResult {
                        contacts: vec![make_contact(
                            "bob",
                            "Bob",
                            crate::domain::TrustLevel::Trusted,
                            true,
                        )],
                        next_cursor: Some("contact:1".to_owned()),
                    },
                    Some("contact:1") => crate::domain::ContactListResult {
                        contacts: vec![make_contact(
                            "charlie",
                            "Charlie",
                            crate::domain::TrustLevel::Untrusted,
                            false,
                        )],
                        next_cursor: None,
                    },
                    _ => crate::domain::ContactListResult {
                        contacts: Vec::new(),
                        next_cursor: None,
                    },
                });
            }
            Ok(crate::domain::ContactListResult {
                contacts: vec![make_contact(
                    "bob",
                    "Bob",
                    crate::domain::TrustLevel::Trusted,
                    true,
                )],
                next_cursor: None,
            })
        }

        fn identity_announce_now(&self) -> Result<Ack, SdkError> {
            Ok(Ack { accepted: true, revision: None })
        }

        fn identity_presence_list(
            &self,
            _req: crate::domain::PresenceListRequest,
        ) -> Result<crate::domain::PresenceListResult, SdkError> {
            let req = _req;
            let bob = crate::domain::PresenceRecord {
                peer_id: "bob".to_owned(),
                last_seen_ts_ms: 200,
                first_seen_ts_ms: 120,
                seen_count: 3,
                name: Some("Bob Relay".to_owned()),
                name_source: Some("announce".to_owned()),
                trust_level: Some(crate::domain::TrustLevel::Trusted),
                bootstrap: Some(true),
                extensions: BTreeMap::from([("source".to_owned(), serde_json::json!("presence"))]),
            };
            let eve = crate::domain::PresenceRecord {
                peer_id: "eve".to_owned(),
                last_seen_ts_ms: 99,
                first_seen_ts_ms: 90,
                seen_count: 1,
                name: Some("Eve".to_owned()),
                name_source: Some("announce".to_owned()),
                trust_level: Some(crate::domain::TrustLevel::Unknown),
                bootstrap: Some(false),
                extensions: BTreeMap::new(),
            };
            if self.paginate_discovery {
                return Ok(match req.cursor.as_deref() {
                    None => crate::domain::PresenceListResult {
                        peers: vec![bob],
                        next_cursor: Some("presence:1".to_owned()),
                    },
                    Some("presence:1") => crate::domain::PresenceListResult {
                        peers: vec![eve],
                        next_cursor: None,
                    },
                    _ => crate::domain::PresenceListResult {
                        peers: Vec::new(),
                        next_cursor: None,
                    },
                });
            }
            Ok(crate::domain::PresenceListResult {
                peers: vec![bob, eve],
                next_cursor: None,
            })
        }

        fn identity_contact_update(
            &self,
            req: crate::domain::ContactUpdateRequest,
        ) -> Result<crate::domain::ContactRecord, SdkError> {
            Ok(crate::domain::ContactRecord {
                identity: req.identity,
                display_name: req.display_name,
                trust_level: req.trust_level.unwrap_or(crate::domain::TrustLevel::Unknown),
                bootstrap: req.bootstrap.unwrap_or(false),
                updated_ts_ms: 500,
                metadata: req.metadata,
                extensions: req.extensions,
            })
        }

        fn identity_bootstrap(
            &self,
            req: crate::domain::IdentityBootstrapRequest,
        ) -> Result<crate::domain::ContactRecord, SdkError> {
            Ok(crate::domain::ContactRecord {
                identity: req.identity,
                display_name: None,
                trust_level: crate::domain::TrustLevel::Trusted,
                bootstrap: true,
                updated_ts_ms: 600,
                metadata: BTreeMap::new(),
                extensions: req.extensions,
            })
        }

        fn command_invoke(
            &self,
            req: crate::domain::RemoteCommandRequest,
        ) -> Result<crate::domain::RemoteCommandResponse, SdkError> {
            self.remote_command_results
                .lock()
                .expect("remote command results")
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(crate::domain::RemoteCommandResponse {
                        accepted: true,
                        payload: serde_json::json!({
                            "command": req.command,
                            "target": req.target,
                            "payload": req.payload,
                        }),
                        extensions: req.extensions,
                    })
                })
        }

        fn envelope_execute(
            &self,
            envelope: crate::app::Envelope,
        ) -> Result<crate::app::EnvelopeResponse, SdkError> {
            self.envelope_results
                .lock()
                .expect("envelope results")
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(crate::app::EnvelopeResponse {
                        operation_id: envelope.operation_id,
                        kind: crate::app::EnvelopeKind::Result,
                        accepted: true,
                        correlation_id: envelope.correlation_id,
                        payload: serde_json::json!({
                            "query": true,
                            "payload": envelope.payload,
                        }),
                        extensions: envelope.extensions,
                    })
                })
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
    fn config_operation_registry_merges_custom_entries() {
        let config = Config::testing_default().with_custom_operation(OperationEntry::new(
            "vendor.example.custom",
            "custom",
            OperationKind::Command,
            TransportVariant::Extension,
            "Custom vendor command.",
        ));
        let registry = config.operation_registry().expect("registry");
        assert!(registry.supports("vendor.example.custom"));
        assert!(registry.supports("sdk_poll_events_v2"));
    }

    #[test]
    fn client_exposes_built_in_registry_before_start() {
        let app = Client::new(MockBackend::new());
        let registry = app.operation_registry().expect("registry");
        assert_eq!(
            registry.canonicalize("sdk_identity_contact_list_v2").expect("canonical id").as_str(),
            "app.contact.list"
        );
    }

    #[test]
    fn execute_envelope_routes_runtime_status_locally() {
        let app = Client::new(MockBackend::new());
        let response = app
            .query("app.runtime.status", serde_json::json!({}))
            .expect("runtime status");
        assert_eq!(response.kind, EnvelopeKind::Result);
        assert_eq!(response.operation_id.as_str(), "app.runtime.status");
        assert_eq!(response.payload.get("state").and_then(|value| value.as_str()), Some("new"));
    }

    #[test]
    fn execute_envelope_routes_identity_queries_to_backend() {
        let app = Client::new(MockBackend::new());
        let response = app
            .query("app.identity.list", serde_json::json!({}))
            .expect("identity list");
        let identities = response.payload.as_array().expect("identity array");
        assert_eq!(identities.len(), 1);
        assert_eq!(
            identities[0]
                .get("display_name")
                .and_then(|value| value.as_str()),
            Some("Alice")
        );
    }

    #[test]
    fn execute_envelope_accepts_registered_aliases() {
        let app = Client::new(MockBackend::new());
        let response = app
            .query("sdk_identity_list_v2", serde_json::json!({}))
            .expect("identity list via alias");
        assert_eq!(response.operation_id.as_str(), "app.identity.list");
        let identities = response.payload.as_array().expect("identity array");
        assert_eq!(identities[0]["identity"], json!("alice"));
    }

    #[test]
    fn execute_envelope_routes_discovery_operations_locally() {
        let app = Client::new(MockBackend::new());

        let announce = app
            .command("sdk_identity_announce_now_v2", serde_json::json!({}))
            .expect("announce");
        assert_eq!(announce.operation_id.as_str(), "app.identity.announce");
        assert_eq!(announce.payload["accepted"], json!(true));

        let presence = app
            .query("sdk_identity_presence_list_v2", serde_json::json!({ "limit": 10 }))
            .expect("presence");
        assert_eq!(presence.operation_id.as_str(), "app.identity.presence.list");
        assert_eq!(presence.payload["peers"].as_array().expect("peer rows").len(), 2);

        let contact = app
            .command(
                "sdk_identity_contact_update_v2",
                serde_json::json!({
                    "identity": "charlie",
                    "display_name": "Charlie",
                    "trust_level": "trusted",
                    "bootstrap": true
                }),
            )
            .expect("contact update");
        assert_eq!(contact.operation_id.as_str(), "app.contact.update");
        assert_eq!(contact.payload["identity"], json!("charlie"));

        let bootstrap = app
            .command(
                "sdk_identity_bootstrap_v2",
                serde_json::json!({ "identity": "delta", "auto_sync": true }),
            )
            .expect("bootstrap");
        assert_eq!(bootstrap.operation_id.as_str(), "app.identity.bootstrap");
        assert_eq!(bootstrap.payload["identity"], json!("delta"));
    }

    #[test]
    fn execute_envelope_routes_runtime_start_and_stop_locally() {
        let app = Client::new(MockBackend::new());
        let start = app
            .command(
                "app.runtime.start",
                serde_json::to_value(Config::testing_default()).expect("config value"),
            )
            .expect("runtime start");
        assert_eq!(start.operation_id.as_str(), "app.runtime.start");
        assert_eq!(
            start.payload.get("profile").and_then(|value| value.as_str()),
            Some("testing_default")
        );

        let stop = app
            .command("app.runtime.stop", serde_json::json!({ "mode": "graceful" }))
            .expect("runtime stop");
        assert_eq!(stop.operation_id.as_str(), "app.runtime.stop");
        assert_eq!(
            stop.payload.get("accepted").and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn execute_envelope_routes_delivery_send_locally() {
        let backend = MockBackend::new();
        backend.queue_send_result(Ok(crate::MessageId("msg-1".to_owned())));
        let app = Client::new(backend);
        app.start(Config::testing_default()).expect("start");

        let response = app
            .command(
                "app.delivery.send",
                serde_json::json!({
                    "source": "src",
                    "destination": "dst",
                    "payload": { "content": "hello" },
                    "correlation_id": "corr-1"
                }),
            )
            .expect("delivery send");
        assert_eq!(response.operation_id.as_str(), "app.delivery.send");
        assert_eq!(
            response.payload.get("message_id").and_then(|value| value.as_str()),
            Some("msg-1")
        );
    }

    #[test]
    fn execute_envelope_routes_custom_commands_via_remote_command_backend() {
        let backend = MockBackend::new();
        backend.queue_remote_command_result(Ok(crate::domain::RemoteCommandResponse {
            accepted: true,
            payload: serde_json::json!({ "ok": true }),
            extensions: BTreeMap::from([("transport".to_owned(), serde_json::json!("remote"))]),
        }));
        let app = Client::new(backend);
        app.start(
            Config::desktop_default().with_custom_operation(OperationEntry::new(
                "vendor.example.custom",
                "custom",
                OperationKind::Command,
                TransportVariant::Extension,
                "Custom vendor command.",
            )),
        )
        .expect("start");
        let response = app
            .command("vendor.example.custom", serde_json::json!({ "value": 1 }))
            .expect("custom command");
        assert_eq!(response.operation_id.as_str(), "vendor.example.custom");
        assert_eq!(response.payload.get("ok").and_then(|value| value.as_bool()), Some(true));
        assert_eq!(
            response
                .extensions
                .get("transport")
                .and_then(|value| value.as_str()),
            Some("remote")
        );
    }

    #[test]
    fn execute_envelope_rejects_kind_mismatches() {
        let app = Client::new(MockBackend::new());
        let err = app
            .execute_envelope(Envelope::command("app.identity.list", serde_json::json!({})))
            .expect_err("kind mismatch should fail");
        assert_eq!(err.code.as_str(), "SDK_APP_VALIDATION_INVALID_ARGUMENT");
    }

    #[test]
    fn execute_envelope_routes_unhandled_queries_to_backend_envelope_path() {
        let backend = MockBackend::new();
        backend.queue_envelope_result(Ok(crate::app::EnvelopeResponse {
            operation_id: crate::app::OperationId::from("app.message.history.list"),
            kind: crate::app::EnvelopeKind::Result,
            accepted: true,
            correlation_id: Some("corr-1".to_owned()),
            payload: serde_json::json!({ "messages": [] }),
            extensions: BTreeMap::from([("via".to_owned(), serde_json::json!("envelope"))]),
        }));
        let app = Client::new(backend);
        let response = app
            .query("app.message.history.list", serde_json::json!({ "limit": 10 }))
            .expect("history query");
        assert_eq!(response.operation_id.as_str(), "app.message.history.list");
        assert_eq!(
            response.extensions.get("via").and_then(|value| value.as_str()),
            Some("envelope")
        );
    }

    #[test]
    fn discovery_helpers_map_backend_identity_contact_and_presence_models() {
        let app = Client::new(MockBackend::new());

        let identities = app.identities().expect("identities");
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].identity, "alice");
        assert_eq!(identities[0].display_name.as_deref(), Some("Alice"));

        let contacts = app.contacts(Some("cursor-1".to_owned()), Some(5)).expect("contacts");
        assert_eq!(contacts.contacts.len(), 1);
        assert_eq!(contacts.contacts[0].identity, "bob");
        assert_eq!(contacts.contacts[0].trust_level, TrustLevel::Trusted);
        assert_eq!(
            contacts.contacts[0]
                .extensions
                .get("cursor")
                .and_then(|value| value.as_str()),
            Some("cursor-1")
        );

        let presence = app.presence(None, Some(10)).expect("presence");
        assert_eq!(presence.peers.len(), 2);
        assert_eq!(presence.peers[0].peer_id, "bob");
        assert_eq!(presence.peers[0].display_name.as_deref(), Some("Bob Relay"));
        assert_eq!(presence.peers[0].trust_level, Some(TrustLevel::Trusted));
        assert!(presence.peers[0].bootstrap.unwrap_or(false));
    }

    #[test]
    fn discovery_helpers_update_contacts_and_bootstrap_identities() {
        let app = Client::new(MockBackend::new());

        let updated = app
            .update_contact(
                super::super::discovery::ContactUpdate::new("charlie")
                    .with_display_name("Charlie")
                    .with_trust_level(TrustLevel::Untrusted)
                    .with_bootstrap(true),
            )
            .expect("contact update");
        assert_eq!(updated.identity, "charlie");
        assert_eq!(updated.display_name.as_deref(), Some("Charlie"));
        assert_eq!(updated.trust_level, TrustLevel::Untrusted);
        assert!(updated.bootstrap);

        let bootstrapped = app
            .bootstrap_identity(super::super::discovery::BootstrapRequest::new("delta"))
            .expect("bootstrap");
        assert_eq!(bootstrapped.identity, "delta");
        assert_eq!(bootstrapped.trust_level, TrustLevel::Trusted);
        assert!(bootstrapped.bootstrap);
    }

    #[test]
    fn peer_directory_merges_contact_and_presence_views() {
        let app = Client::new(MockBackend::new());
        let peers = app.peer_directory(Some(10)).expect("peer directory");

        assert_eq!(peers.len(), 2);

        let bob = peers.iter().find(|entry| entry.peer_id == "bob").expect("bob entry");
        assert_eq!(bob.display_name.as_deref(), Some("Bob"));
        assert_eq!(bob.name_source.as_deref(), Some("contact"));
        assert_eq!(bob.trust_level, Some(TrustLevel::Trusted));
        assert!(bob.online);
        assert!(bob.bootstrap);
        assert_eq!(bob.last_seen_ts_ms, Some(200));
        assert_eq!(bob.first_seen_ts_ms, Some(120));
        assert_eq!(bob.seen_count, 3);

        let eve = peers.iter().find(|entry| entry.peer_id == "eve").expect("eve entry");
        assert_eq!(eve.display_name.as_deref(), Some("Eve"));
        assert_eq!(eve.name_source.as_deref(), Some("announce"));
        assert_eq!(eve.trust_level, Some(TrustLevel::Unknown));
        assert!(eve.online);
        assert!(!eve.bootstrap);
    }

    #[test]
    fn peer_directory_consumes_all_contact_and_presence_pages() {
        let app = Client::new(MockBackend::new_paginated());
        let peers = app.peer_directory(None).expect("peer directory");

        assert_eq!(peers.len(), 3);
        assert!(peers.iter().any(|entry| entry.peer_id == "bob"));
        assert!(peers.iter().any(|entry| entry.peer_id == "charlie"));
        assert!(peers.iter().any(|entry| entry.peer_id == "eve"));
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
        assert_eq!(
            report.attempts[0].disposition,
            super::super::delivery::AttemptDisposition::Retried
        );
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
                    queue_pressure_strategy: Some(
                        super::super::delivery::QueuePressureStrategy::FailFast,
                    ),
                    ..Default::default()
                },
            )
            .expect_err("queue pressure should fail fast");

        assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_QUEUE_PRESSURE");
    }

    #[test]
    fn send_with_options_maps_retry_exhaustion() {
        let backend = MockBackend::new();
        backend.queue_send_result(Err(SdkError::new(
            code::INTERNAL,
            SdkErrorCategory::Internal,
            "temporary",
        )
        .with_retryable(true)));
        backend.queue_send_result(Err(SdkError::new(
            code::INTERNAL,
            SdkErrorCategory::Internal,
            "temporary",
        )
        .with_retryable(true)));
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
