use crate::error::SdkError;
use crate::event::{EventBatch, EventCursor};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot, MessageId, RuntimeSnapshot,
    SendRequest, ShutdownMode, StartRequest, TickBudget, TickResult,
};

pub trait LxmfSdk {
    fn start(&self, req: StartRequest) -> Result<ClientHandle, SdkError>;
    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError>;
    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError>;
    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError>;
    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError>;
    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError>;
    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError>;
    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError>;
}

pub trait LxmfSdkManualTick {
    fn tick(&self, budget: TickBudget) -> Result<TickResult, SdkError>;
}

#[cfg(feature = "sdk-async")]
pub trait LxmfSdkAsync {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;
}

#[cfg(not(feature = "sdk-async"))]
pub trait LxmfSdkAsync {}
