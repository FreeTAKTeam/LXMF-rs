#![allow(clippy::result_large_err)]

mod api;
pub mod app;
mod backend;
pub mod capability;
mod client;
pub mod domain;
mod error;
pub mod event;
mod lifecycle;
#[cfg(feature = "std")]
pub mod messaging;
pub mod profiles;
pub mod types;

// Stability class: stable
pub use api::{LxmfSdk, LxmfSdkAsync, LxmfSdkManualTick};
// Stability class: experimental (capability-gated extension traits)
pub use api::{
    LxmfSdkAttachments, LxmfSdkGroupDelivery, LxmfSdkIdentity, LxmfSdkMarkers, LxmfSdkOperations,
    LxmfSdkPaper, LxmfSdkRemoteCommands, LxmfSdkTelemetry, LxmfSdkTopics, LxmfSdkVoiceSignaling,
};
// Stability class: internal (backend composition surface)
pub use backend::mobile_ble::{
    validate_capabilities as validate_mobile_ble_capabilities,
    validate_event_payload_bounds as validate_mobile_ble_event_payload_bounds,
    validate_event_sequence as validate_mobile_ble_event_sequence, MobileBleCapabilities,
    MobileBleConnectRequest, MobileBleEvent, MobileBleEventKind, MobileBleHostAdapter,
    MobileBleReadRequest, MobileBleReadResult, MobileBleSessionDescriptor, MobileBleWriteAck,
    MobileBleWriteRequest,
};
#[cfg(all(feature = "rpc-backend", feature = "std"))]
pub use backend::rpc::RpcBackendClient;
#[cfg(all(feature = "zmq-pipeline-backend", feature = "std"))]
pub use backend::zmq_pipeline::{
    ZmqEndpointRole, ZmqPipelineBackendClient, ZmqPipelineBackendConfig, ZmqPipelineTokenAuth,
};
pub use backend::{
    KeyProviderClass, SdkBackend, SdkBackendAsyncEvents, SdkBackendKeyManagement, SdkKeyPurpose,
    SdkStoredKey,
};
#[cfg(feature = "sdk-async")]
pub use backend::{SdkBackendAsyncOps, SdkBoxFuture, SdkEventStream};
// Stability class: stable
pub use capability::{
    effective_capabilities_for_profile, negotiate_contract_version, negotiate_plugins,
    CapabilityDescriptor, CapabilityState, EffectiveLimits, NegotiationRequest,
    NegotiationResponse, PluginDescriptor, PluginState,
};
// Stability class: internal
pub use client::Client;
// Stability class: stable
pub use domain::{
    AttachmentDownloadChunk, AttachmentDownloadChunkRequest, AttachmentId, AttachmentListRequest,
    AttachmentListResult, AttachmentMeta, AttachmentStoreRequest, AttachmentUploadChunkAck,
    AttachmentUploadChunkRequest, AttachmentUploadCommitRequest, AttachmentUploadId,
    AttachmentUploadSession, AttachmentUploadStartRequest, ContactListRequest, ContactListResult,
    ContactRecord, ContactUpdateRequest, GeoPoint, IdentityBootstrapRequest, IdentityBundle,
    IdentityImportRequest, IdentityRef, IdentityResolveRequest, MarkerCreateRequest,
    MarkerDeleteRequest, MarkerId, MarkerListRequest, MarkerListResult, MarkerRecord,
    MarkerUpdatePositionRequest, PaperMessageEnvelope, PresenceListRequest, PresenceListResult,
    PresenceRecord, RemoteCommandRequest, RemoteCommandResponse, RemoteCommandSession,
    RemoteCommandSessionListRequest, RemoteCommandSessionListResult, TelemetryPoint,
    TelemetryQuery, TopicCreateRequest, TopicId, TopicListRequest, TopicListResult, TopicPath,
    TopicPublishRequest, TopicRecord, TopicSubscriptionRequest, TrustLevel, VoiceSessionId,
    VoiceSessionOpenRequest, VoiceSessionState, VoiceSessionUpdateRequest,
};
pub use error::{code as error_code, ErrorCategory, ErrorDetails, SdkError};
// Stability class: stable
pub use event::{
    EventBatch, EventCursor, EventSubscription, SdkEvent, Severity, SubscriptionStart,
};
// Stability class: stable
pub use lifecycle::{Lifecycle, SdkMethod};
#[cfg(feature = "std")]
pub use messaging::{
    AnnounceRecord, ConversationRecord, MessageDirection, MessageMethod, MessageRecord,
    MessageState, MessagingStore, PeerRecord, PeerState, SendMessageRequest, StoredOutboundMessage,
    SyncPhase, SyncStatus,
};
pub use profiles::{
    default_effective_limits, default_memory_budget, required_capabilities, supports_capability,
    MemoryBudget,
};
// Stability class: stable
pub use types::{
    Ack, AuthMode, BindMode, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot,
    DeliveryState, EventSinkConfig, EventSinkKind, EventSinkPatch, EventStreamConfig,
    GroupRecipientState, GroupSendOutcome, GroupSendRequest, GroupSendResult, MessageId,
    OverflowPolicy, Profile, RedactionConfig, RedactionTransform, RpcBackendConfig,
    RuntimeSnapshot, RuntimeState, SdkConfig, SendRequest, ShutdownMode, StartRequest,
    StoreForwardCapacityPolicy, StoreForwardConfig, StoreForwardEvictionPriority,
    StoreForwardPatch, TickBudget, TickResult,
};

pub const CONTRACT_RELEASE: &str = "v2.5";
pub const SCHEMA_NAMESPACE: &str = "v2";
