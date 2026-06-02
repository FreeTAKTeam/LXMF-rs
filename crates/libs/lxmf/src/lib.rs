//! Umbrella crate for LXMF-rs library consumers.

#[cfg(feature = "wire")]
pub use lxmf_core as wire;
#[cfg(feature = "sdk")]
pub use lxmf_sdk as sdk;

#[cfg(feature = "wire")]
pub use lxmf_core::{
    decide_delivery, DeliveryDecision, LxmfError, Message, MessageMethod, MessageState, Payload,
    TransportMethod, WireMessage,
};
#[cfg(feature = "sdk")]
pub use lxmf_sdk::{
    error_code, Client, ClientHandle, EventBatch, EventCursor, LxmfSdk, LxmfSdkAsync,
    LxmfSdkManualTick, RuntimeSnapshot, RuntimeState, SdkConfig, SdkError, SendRequest,
    StartRequest, TickResult,
};
