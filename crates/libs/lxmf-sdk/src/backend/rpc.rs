use crate::backend::SdkBackend;
#[cfg(feature = "sdk-async")]
use crate::backend::SdkBackendAsyncEvents;
use crate::capability::{EffectiveLimits, NegotiationRequest, NegotiationResponse};
use crate::domain::{
    AttachmentId, AttachmentListRequest, AttachmentListResult, AttachmentMeta,
    AttachmentStoreRequest, IdentityBundle, IdentityImportRequest, IdentityRef,
    IdentityResolveRequest, MarkerCreateRequest, MarkerId, MarkerListRequest, MarkerListResult,
    MarkerRecord, MarkerUpdatePositionRequest, PaperMessageEnvelope, RemoteCommandRequest,
    RemoteCommandResponse, TelemetryPoint, TelemetryQuery, TopicCreateRequest, TopicId,
    TopicListRequest, TopicListResult, TopicPublishRequest, TopicRecord, TopicSubscriptionRequest,
    VoiceSessionId, VoiceSessionOpenRequest, VoiceSessionState, VoiceSessionUpdateRequest,
};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor, SdkEvent, Severity};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, AuthMode, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState, MessageId,
    RuntimeSnapshot, RuntimeState, SendRequest, ShutdownMode, TickBudget, TickResult,
};
use serde::de::DeserializeOwned;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "rpc/core_impl.rs"]
mod core_impl;
#[path = "rpc/domains_impl.rs"]
mod domains_impl;
#[path = "rpc/parsing.rs"]
mod parsing;
#[path = "rpc/transport.rs"]
mod transport;

pub struct RpcBackendClient {
    endpoint: String,
    next_request_id: AtomicU64,
    negotiated_capabilities: RwLock<Vec<String>>,
    session_auth: RwLock<SessionAuth>,
}

#[derive(Clone, Debug)]
enum SessionAuth {
    LocalTrusted,
    Token { issuer: String, audience: String, shared_secret: String, ttl_secs: u64 },
    Mtls { allowed_san: Option<String> },
}

impl RpcBackendClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            next_request_id: AtomicU64::new(1),
            negotiated_capabilities: RwLock::new(Vec::new()),
            session_auth: RwLock::new(SessionAuth::LocalTrusted),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn now_seconds() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
    }

    fn has_capability(&self, capability_id: &str) -> bool {
        self.negotiated_capabilities
            .read()
            .expect("negotiated_capabilities rwlock poisoned")
            .iter()
            .any(|capability| capability == capability_id)
    }
}

impl SdkBackend for RpcBackendClient {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        self.negotiate_impl(req)
    }

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        self.send_impl(req)
    }

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        self.cancel_impl(id)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        self.status_impl(id)
    }

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError> {
        self.configure_impl(expected_revision, patch)
    }

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError> {
        self.poll_events_impl(cursor, max)
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        self.snapshot_impl()
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        self.shutdown_impl(mode)
    }

    fn topic_create(&self, req: TopicCreateRequest) -> Result<TopicRecord, SdkError> {
        self.topic_create_impl(req)
    }

    fn topic_get(&self, topic_id: TopicId) -> Result<Option<TopicRecord>, SdkError> {
        self.topic_get_impl(topic_id)
    }

    fn topic_list(&self, req: TopicListRequest) -> Result<TopicListResult, SdkError> {
        self.topic_list_impl(req)
    }

    fn topic_subscribe(&self, req: TopicSubscriptionRequest) -> Result<Ack, SdkError> {
        self.topic_subscribe_impl(req)
    }

    fn topic_unsubscribe(&self, topic_id: TopicId) -> Result<Ack, SdkError> {
        self.topic_unsubscribe_impl(topic_id)
    }

    fn topic_publish(&self, req: TopicPublishRequest) -> Result<Ack, SdkError> {
        self.topic_publish_impl(req)
    }

    fn telemetry_query(&self, query: TelemetryQuery) -> Result<Vec<TelemetryPoint>, SdkError> {
        self.telemetry_query_impl(query)
    }

    fn telemetry_subscribe(&self, query: TelemetryQuery) -> Result<Ack, SdkError> {
        self.telemetry_subscribe_impl(query)
    }

    fn attachment_store(&self, req: AttachmentStoreRequest) -> Result<AttachmentMeta, SdkError> {
        self.attachment_store_impl(req)
    }

    fn attachment_get(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<Option<AttachmentMeta>, SdkError> {
        self.attachment_get_impl(attachment_id)
    }

    fn attachment_list(
        &self,
        req: AttachmentListRequest,
    ) -> Result<AttachmentListResult, SdkError> {
        self.attachment_list_impl(req)
    }

    fn attachment_delete(&self, attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        self.attachment_delete_impl(attachment_id)
    }

    fn attachment_download(&self, attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        self.attachment_download_impl(attachment_id)
    }

    fn attachment_associate_topic(
        &self,
        attachment_id: AttachmentId,
        topic_id: TopicId,
    ) -> Result<Ack, SdkError> {
        self.attachment_associate_topic_impl(attachment_id, topic_id)
    }

    fn marker_create(&self, req: MarkerCreateRequest) -> Result<MarkerRecord, SdkError> {
        self.marker_create_impl(req)
    }

    fn marker_list(&self, req: MarkerListRequest) -> Result<MarkerListResult, SdkError> {
        self.marker_list_impl(req)
    }

    fn marker_update_position(
        &self,
        req: MarkerUpdatePositionRequest,
    ) -> Result<MarkerRecord, SdkError> {
        self.marker_update_position_impl(req)
    }

    fn marker_delete(&self, marker_id: MarkerId) -> Result<Ack, SdkError> {
        self.marker_delete_impl(marker_id)
    }

    fn identity_list(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        self.identity_list_impl()
    }

    fn identity_activate(&self, identity: IdentityRef) -> Result<Ack, SdkError> {
        self.identity_activate_impl(identity)
    }

    fn identity_import(&self, req: IdentityImportRequest) -> Result<IdentityBundle, SdkError> {
        self.identity_import_impl(req)
    }

    fn identity_export(&self, identity: IdentityRef) -> Result<IdentityImportRequest, SdkError> {
        self.identity_export_impl(identity)
    }

    fn identity_resolve(
        &self,
        req: IdentityResolveRequest,
    ) -> Result<Option<IdentityRef>, SdkError> {
        self.identity_resolve_impl(req)
    }

    fn paper_encode(&self, message_id: MessageId) -> Result<PaperMessageEnvelope, SdkError> {
        self.paper_encode_impl(message_id)
    }

    fn paper_decode(&self, envelope: PaperMessageEnvelope) -> Result<Ack, SdkError> {
        self.paper_decode_impl(envelope)
    }

    fn command_invoke(&self, req: RemoteCommandRequest) -> Result<RemoteCommandResponse, SdkError> {
        self.command_invoke_impl(req)
    }

    fn command_reply(
        &self,
        correlation_id: String,
        reply: RemoteCommandResponse,
    ) -> Result<Ack, SdkError> {
        self.command_reply_impl(correlation_id, reply)
    }

    fn voice_session_open(&self, req: VoiceSessionOpenRequest) -> Result<VoiceSessionId, SdkError> {
        self.voice_session_open_impl(req)
    }

    fn voice_session_update(
        &self,
        req: VoiceSessionUpdateRequest,
    ) -> Result<VoiceSessionState, SdkError> {
        self.voice_session_update_impl(req)
    }

    fn voice_session_close(&self, session_id: VoiceSessionId) -> Result<Ack, SdkError> {
        self.voice_session_close_impl(session_id)
    }

    fn tick(&self, budget: TickBudget) -> Result<TickResult, SdkError> {
        self.tick_impl(budget)
    }
}

#[cfg(feature = "sdk-async")]
impl SdkBackendAsyncEvents for RpcBackendClient {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        self.subscribe_events_impl(start)
    }
}
