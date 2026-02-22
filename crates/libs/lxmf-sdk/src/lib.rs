#![allow(clippy::result_large_err)]

mod api;
mod backend;
pub mod capability;
mod client;
pub mod domain;
mod error;
pub mod event;
mod lifecycle;
pub mod profiles;
pub mod types;

// Stability class: stable
pub use api::{LxmfSdk, LxmfSdkAsync, LxmfSdkManualTick};
// Stability class: experimental (capability-gated extension traits)
pub use api::{
    LxmfSdkAttachments, LxmfSdkGroupDelivery, LxmfSdkIdentity, LxmfSdkMarkers, LxmfSdkPaper,
    LxmfSdkRemoteCommands, LxmfSdkTelemetry, LxmfSdkTopics, LxmfSdkVoiceSignaling,
};
// Stability class: internal (backend composition surface)
#[cfg(all(feature = "rpc-backend", feature = "std"))]
pub use backend::rpc::RpcBackendClient;
pub use backend::{
    KeyProviderClass, SdkBackend, SdkBackendAsyncEvents, SdkBackendKeyManagement, SdkKeyPurpose,
    SdkStoredKey,
};
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
    PresenceRecord, RemoteCommandRequest, RemoteCommandResponse, TelemetryPoint, TelemetryQuery,
    TopicCreateRequest, TopicId, TopicListRequest, TopicListResult, TopicPath, TopicPublishRequest,
    TopicRecord, TopicSubscriptionRequest, TrustLevel, VoiceSessionId, VoiceSessionOpenRequest,
    VoiceSessionState, VoiceSessionUpdateRequest,
};
pub use error::{code as error_code, ErrorCategory, ErrorDetails, SdkError};
// Stability class: stable
pub use event::{
    EventBatch, EventCursor, EventSubscription, SdkEvent, Severity, SubscriptionStart,
};
// Stability class: stable
pub use lifecycle::{Lifecycle, SdkMethod};
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
