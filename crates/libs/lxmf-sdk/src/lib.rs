pub mod api;
pub mod backend;
pub mod capability;
pub mod error;
pub mod event;
pub mod lifecycle;
pub mod profiles;
pub mod types;

pub use api::{LxmfSdk, LxmfSdkAsync, LxmfSdkManualTick};
pub use backend::{SdkBackend, SdkBackendAsyncEvents};
pub use capability::{
    effective_capabilities_for_profile, negotiate_contract_version, CapabilityDescriptor,
    CapabilityState, EffectiveLimits, NegotiationRequest, NegotiationResponse,
};
pub use error::{code as error_code, ErrorCategory, ErrorDetails, SdkError};
pub use event::{
    EventBatch, EventCursor, EventSubscription, SdkEvent, Severity, SubscriptionStart,
};
pub use lifecycle::{Lifecycle, SdkMethod};
pub use profiles::{default_effective_limits, required_capabilities, supports_capability};
pub use types::{
    Ack, AuthMode, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot, DeliveryState,
    EventStreamConfig, MessageId, Profile, RedactionConfig, RedactionTransform, RpcBackendConfig,
    RuntimeSnapshot, RuntimeState, SdkConfig, SendRequest, ShutdownMode, StartRequest, TickBudget,
    TickResult,
};

pub const CONTRACT_RELEASE: &str = "v2.5";
pub const SCHEMA_NAMESPACE: &str = "v2";
