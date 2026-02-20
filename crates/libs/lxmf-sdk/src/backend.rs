use crate::capability::{NegotiationRequest, NegotiationResponse};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, MessageId, RuntimeSnapshot, SendRequest,
    ShutdownMode, TickBudget, TickResult,
};

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
}

#[cfg(feature = "sdk-async")]
pub trait SdkBackendAsyncEvents: SdkBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;
}

#[cfg(not(feature = "sdk-async"))]
pub trait SdkBackendAsyncEvents: SdkBackend {}
