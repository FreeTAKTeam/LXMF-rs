#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod constants;
mod error;

pub mod errors;
pub mod identity;
pub mod inbound_decode;
pub mod message;
pub mod payload_fields;
#[cfg(feature = "std")]
pub mod wire_fields;

pub use error::LxmfError;
pub use message::{Message, Payload, WireMessage};
