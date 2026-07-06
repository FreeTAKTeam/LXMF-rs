use crate::app::{Envelope, EnvelopeResponse, OperationRegistry};
use crate::capability::{NegotiationRequest, NegotiationResponse};
use crate::domain::{
    AttachmentDownloadChunk, AttachmentDownloadChunkRequest, AttachmentId, AttachmentListRequest,
    AttachmentListResult, AttachmentMeta, AttachmentStoreRequest, AttachmentUploadChunkAck,
    AttachmentUploadChunkRequest, AttachmentUploadCommitRequest, AttachmentUploadSession,
    AttachmentUploadStartRequest, ContactListRequest, ContactListResult, ContactRecord,
    ContactUpdateRequest, IdentityAnnounceRequest, IdentityAnnounceResult,
    IdentityBootstrapRequest, IdentityBundle, IdentityImportRequest, IdentityRef,
    IdentityResolveRequest, MarkerCreateRequest, MarkerDeleteRequest, MarkerListRequest,
    MarkerListResult, MarkerRecord, MarkerUpdatePositionRequest, PaperDecodeResult,
    PaperMessageEnvelope, PeerConnectionRequest, PeerConnectionResult, PresenceListRequest,
    PresenceListResult, RemoteCommandRequest, RemoteCommandResponse, RemoteCommandSession,
    RemoteCommandSessionListRequest, RemoteCommandSessionListResult, TelemetryPoint,
    TelemetryQuery, TopicCreateRequest, TopicId, TopicListRequest, TopicListResult,
    TopicPublishRequest, TopicRecord, TopicSubscriptionRequest, VoiceSessionId,
    VoiceSessionOpenRequest, VoiceSessionState, VoiceSessionUpdateRequest,
};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SdkEvent, SubscriptionStart};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, MessageId, RuntimeSnapshot, SendRequest,
    ShutdownMode, TickBudget, TickResult,
};
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
use serde_json::Map as JsonMap;
#[cfg(any(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
use serde_json::Value as JsonValue;
#[cfg(feature = "sdk-async")]
use std::future::Future;
#[cfg(feature = "sdk-async")]
use std::pin::Pin;
#[cfg(feature = "sdk-async")]
use tokio_stream::Stream;

const CAP_KEY_MANAGEMENT: &str = "sdk.capability.key_management";
#[cfg(any(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
const LXMF_RAW_FIELDS_KEY: &str = "_lxmf_fields_msgpack_b64";

#[cfg(any(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
fn lxmf_wire_fields_from_payload(payload: JsonValue) -> JsonValue {
    let JsonValue::Object(mut map) = payload else {
        return JsonValue::Null;
    };

    if let Some(JsonValue::Object(fields)) = map.remove("fields") {
        return non_empty_fields(fields);
    }

    let fields = map
        .into_iter()
        .filter(|(key, _)| !is_reserved_payload_field_key(key))
        .collect::<JsonMap<String, JsonValue>>();
    non_empty_fields(fields)
}

#[cfg(any(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
fn is_reserved_payload_field_key(key: &str) -> bool {
    key != LXMF_RAW_FIELDS_KEY
        && matches!(
            key,
            "title" | "content" | "body" | "payload" | "_lxmf" | "_sdk" | "_fields_raw"
        )
}

#[cfg(any(feature = "rpc-backend", feature = "zmq-pipeline-backend"))]
fn non_empty_fields(fields: JsonMap<String, JsonValue>) -> JsonValue {
    if fields.is_empty() {
        JsonValue::Null
    } else {
        JsonValue::Object(fields)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyProviderClass {
    InMemory,
    File,
    OsKeystore,
    Hsm,
    Custom(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SdkKeyPurpose {
    IdentitySigning,
    TransportDh,
    SharedSecret,
    Custom(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SdkStoredKey {
    pub key_id: String,
    pub purpose: SdkKeyPurpose,
    pub material: Vec<u8>,
}

pub trait SdkBackend: Send + Sync {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError>;

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError>;

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError>;

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError>;

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError>;

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError>;

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError>;

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError>;

    fn tick(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
        Err(SdkError::new(
            code::CAPABILITY_DISABLED,
            ErrorCategory::Capability,
            "backend does not support manual ticking",
        ))
    }

    fn topic_create(&self, _req: TopicCreateRequest) -> Result<TopicRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topics"))
    }

    fn topic_get(&self, _topic_id: TopicId) -> Result<Option<TopicRecord>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topics"))
    }

    fn topic_list(&self, _req: TopicListRequest) -> Result<TopicListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topics"))
    }

    fn topic_subscribe(&self, _req: TopicSubscriptionRequest) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topic_subscriptions"))
    }

    fn topic_unsubscribe(&self, _topic_id: TopicId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topic_subscriptions"))
    }

    fn topic_publish(&self, _req: TopicPublishRequest) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topic_fanout"))
    }

    fn telemetry_query(&self, _query: TelemetryQuery) -> Result<Vec<TelemetryPoint>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.telemetry_query"))
    }

    fn telemetry_subscribe(&self, _query: TelemetryQuery) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.telemetry_stream"))
    }

    fn attachment_store(&self, _req: AttachmentStoreRequest) -> Result<AttachmentMeta, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_get(
        &self,
        _attachment_id: AttachmentId,
    ) -> Result<Option<AttachmentMeta>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_list(
        &self,
        _req: AttachmentListRequest,
    ) -> Result<AttachmentListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_delete(&self, _attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_delete"))
    }

    fn attachment_download(&self, _attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_upload_start(
        &self,
        _req: AttachmentUploadStartRequest,
    ) -> Result<AttachmentUploadSession, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_upload_chunk(
        &self,
        _req: AttachmentUploadChunkRequest,
    ) -> Result<AttachmentUploadChunkAck, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_upload_commit(
        &self,
        _req: AttachmentUploadCommitRequest,
    ) -> Result<AttachmentMeta, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_download_chunk(
        &self,
        _req: AttachmentDownloadChunkRequest,
    ) -> Result<AttachmentDownloadChunk, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_associate_topic(
        &self,
        _attachment_id: AttachmentId,
        _topic_id: TopicId,
    ) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn marker_create(&self, _req: MarkerCreateRequest) -> Result<MarkerRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn marker_list(&self, _req: MarkerListRequest) -> Result<MarkerListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn marker_update_position(
        &self,
        _req: MarkerUpdatePositionRequest,
    ) -> Result<MarkerRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn marker_delete(&self, _req: MarkerDeleteRequest) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn identity_list(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_multi"))
    }

    fn identity_announce_now(&self) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_discovery"))
    }

    fn identity_announce(
        &self,
        _req: IdentityAnnounceRequest,
    ) -> Result<IdentityAnnounceResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_discovery"))
    }

    fn identity_presence_list(
        &self,
        _req: PresenceListRequest,
    ) -> Result<PresenceListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_discovery"))
    }

    fn identity_activate(&self, _identity: IdentityRef) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_multi"))
    }

    fn identity_import(&self, _req: IdentityImportRequest) -> Result<IdentityBundle, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_import_export"))
    }

    fn identity_export(&self, _identity: IdentityRef) -> Result<IdentityImportRequest, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_import_export"))
    }

    fn identity_resolve(
        &self,
        _req: IdentityResolveRequest,
    ) -> Result<Option<IdentityRef>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_hash_resolution"))
    }

    fn identity_contact_update(
        &self,
        _req: ContactUpdateRequest,
    ) -> Result<ContactRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.contact_management"))
    }

    fn identity_contact_list(
        &self,
        _req: ContactListRequest,
    ) -> Result<ContactListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.contact_management"))
    }

    fn identity_bootstrap(
        &self,
        _req: IdentityBootstrapRequest,
    ) -> Result<ContactRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.contact_management"))
    }

    fn peer_connect(&self, _req: PeerConnectionRequest) -> Result<PeerConnectionResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.peer_lifecycle"))
    }

    fn peer_disconnect(
        &self,
        _req: PeerConnectionRequest,
    ) -> Result<PeerConnectionResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.peer_lifecycle"))
    }

    fn peer_reconnect(
        &self,
        _req: PeerConnectionRequest,
    ) -> Result<PeerConnectionResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.peer_lifecycle"))
    }

    fn paper_encode(&self, _message_id: MessageId) -> Result<PaperMessageEnvelope, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }

    fn paper_decode(&self, _envelope: PaperMessageEnvelope) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }

    fn paper_decode_with_metadata(
        &self,
        _envelope: PaperMessageEnvelope,
    ) -> Result<PaperDecodeResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }

    fn command_invoke(
        &self,
        _req: RemoteCommandRequest,
    ) -> Result<RemoteCommandResponse, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.remote_commands"))
    }

    fn command_reply(
        &self,
        _correlation_id: String,
        _reply: RemoteCommandResponse,
    ) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.remote_commands"))
    }

    fn command_session_get(
        &self,
        _correlation_id: String,
    ) -> Result<Option<RemoteCommandSession>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.remote_commands"))
    }

    fn command_session_list(
        &self,
        _req: RemoteCommandSessionListRequest,
    ) -> Result<RemoteCommandSessionListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.remote_commands"))
    }

    fn voice_session_open(
        &self,
        _req: VoiceSessionOpenRequest,
    ) -> Result<VoiceSessionId, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.voice_signaling"))
    }

    fn voice_session_update(
        &self,
        _req: VoiceSessionUpdateRequest,
    ) -> Result<VoiceSessionState, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.voice_signaling"))
    }

    fn voice_session_close(&self, _session_id: VoiceSessionId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.voice_signaling"))
    }

    fn operation_registry(&self) -> Result<OperationRegistry, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.operation_registry"))
    }

    fn envelope_execute(&self, _envelope: Envelope) -> Result<EnvelopeResponse, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.operation_registry"))
    }
}

pub trait SdkBackendKeyManagement: SdkBackend {
    fn key_provider_class(&self) -> Result<KeyProviderClass, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_get(&self, _key_id: &str) -> Result<Option<SdkStoredKey>, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_put(&self, _key: SdkStoredKey) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_delete(&self, _key_id: &str) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_list_ids(&self) -> Result<Vec<String>, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }
}

#[cfg(feature = "sdk-async")]
pub type SdkEventStream = Pin<Box<dyn Stream<Item = Result<SdkEvent, SdkError>> + Send>>;

#[cfg(feature = "sdk-async")]
pub type SdkBoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SdkError>> + Send + 'a>>;

#[cfg(feature = "sdk-async")]
pub trait SdkBackendAsyncOps: SdkBackend {
    fn negotiate_async(&self, req: NegotiationRequest) -> SdkBoxFuture<'_, NegotiationResponse>;

    fn send_async(&self, req: SendRequest) -> SdkBoxFuture<'_, MessageId>;

    fn status_async(&self, id: MessageId) -> SdkBoxFuture<'_, Option<DeliverySnapshot>>;

    fn snapshot_async(&self) -> SdkBoxFuture<'_, RuntimeSnapshot>;

    fn shutdown_async(&self, mode: ShutdownMode) -> SdkBoxFuture<'_, Ack>;
}

#[cfg(feature = "sdk-async")]
pub trait SdkBackendAsyncEvents: SdkBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        Ok(None)
    }
}

#[cfg(not(feature = "sdk-async"))]
pub trait SdkBackendAsyncEvents: SdkBackend {}

pub mod mobile_ble;

#[cfg(all(feature = "rpc-backend", feature = "std"))]
pub mod rpc;

#[cfg(all(feature = "zmq-pipeline-backend", feature = "std"))]
pub mod zmq_pipeline;

#[cfg(test)]
mod tests {
    include!("backend_tests.rs");
}
