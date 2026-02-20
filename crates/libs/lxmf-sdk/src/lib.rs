#![cfg_attr(not(feature = "std"), no_std)]

pub const CONTRACT_RELEASE: &str = "v2.5";
pub const SCHEMA_NAMESPACE: &str = "v2";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartRequest;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientHandle;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendRequest;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeliverySnapshot;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigPatch;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ack;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventCursor;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventBatch;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSnapshot;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShutdownMode;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickBudget;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickResult;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventSubscription;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubscriptionStart;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CancelResult;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdkError;

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

pub trait LxmfSdkAsync {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;
}
