#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod constants;
mod error;

pub mod announce;
pub mod errors;
pub mod identity;
pub mod inbound_decode;
pub mod message;
pub mod payload_fields;
#[cfg(feature = "std")]
pub mod wire_fields;

pub use error::LxmfError;
pub use message::{
    decide_delivery, DeliveryDecision, Message, MessageMethod, MessageState, Payload,
    TransportMethod, WireMessage,
};
