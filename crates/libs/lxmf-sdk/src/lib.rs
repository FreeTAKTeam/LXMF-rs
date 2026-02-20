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

pub use api::{
    LxmfSdk, LxmfSdkAsync, LxmfSdkAttachments, LxmfSdkIdentity, LxmfSdkManualTick, LxmfSdkMarkers,
    LxmfSdkPaper, LxmfSdkRemoteCommands, LxmfSdkTelemetry, LxmfSdkTopics, LxmfSdkVoiceSignaling,
};
#[cfg(all(feature = "rpc-backend", feature = "std"))]
pub use backend::rpc::RpcBackendClient;
pub use backend::{SdkBackend, SdkBackendAsyncEvents};
pub use capability::{
    effective_capabilities_for_profile, negotiate_contract_version, CapabilityDescriptor,
    CapabilityState, EffectiveLimits, NegotiationRequest, NegotiationResponse,
};
pub use client::Client;
pub use domain::{
    AttachmentId, AttachmentListRequest, AttachmentListResult, AttachmentMeta,
    AttachmentStoreRequest, GeoPoint, IdentityBundle, IdentityImportRequest, IdentityRef,
    IdentityResolveRequest, MarkerCreateRequest, MarkerId, MarkerListRequest, MarkerListResult,
    MarkerRecord, MarkerUpdatePositionRequest, PaperMessageEnvelope, RemoteCommandRequest,
    RemoteCommandResponse, TelemetryPoint, TelemetryQuery, TopicCreateRequest, TopicId,
    TopicListRequest, TopicListResult, TopicPath, TopicPublishRequest, TopicRecord,
    TopicSubscriptionRequest, VoiceSessionId, VoiceSessionOpenRequest, VoiceSessionState,
    VoiceSessionUpdateRequest,
};
pub use error::{code as error_code, ErrorCategory, ErrorDetails, SdkError};
pub use event::{
    EventBatch, EventCursor, EventSubscription, SdkEvent, Severity, SubscriptionStart,
};
pub use lifecycle::{Lifecycle, SdkMethod};
pub use profiles::{
    default_effective_limits, default_memory_budget, required_capabilities, supports_capability,
    MemoryBudget,
};
pub use types::{
    Ack, AuthMode, BindMode, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot,
    DeliveryState, EventStreamConfig, MessageId, OverflowPolicy, Profile, RedactionConfig,
    RedactionTransform, RpcBackendConfig, RuntimeSnapshot, RuntimeState, SdkConfig, SendRequest,
    ShutdownMode, StartRequest, TickBudget, TickResult,
};

pub const CONTRACT_RELEASE: &str = "v2.5";
pub const SCHEMA_NAMESPACE: &str = "v2";
