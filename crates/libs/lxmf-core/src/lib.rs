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
pub mod stamp;
#[cfg(feature = "std")]
pub mod wire_fields;

pub use announce::{
    compression_support_from_app_data, display_name_from_app_data, pn_announce_data_is_valid,
    pn_name_from_app_data, pn_stamp_cost_from_app_data, stamp_cost_from_app_data,
    validate_pn_announce_data, PnAnnounceParseError,
};
pub use error::LxmfError;
pub use message::{
    decide_delivery, DeliveryDecision, Message, MessageMethod, MessageState, Payload,
    TransportMethod, WireMessage,
};
