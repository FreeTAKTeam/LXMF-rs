//! LXMF wire formats, message primitives, and identity helpers.
//!
//! # Standard-library requirement
//!
//! This crate currently requires `std`, including when default features are
//! disabled. Its MessagePack and Reticulum dependencies are not a std-free
//! dependency graph, so `no_std`-only targets are not supported.
//!
//! The default `std` feature controls JSON/file helpers, system-clock timestamps,
//! and standard error implementations. Disabling it retains the existing `0.0`
//! automatic timestamp fallback; it does not remove the dependency on `std`.
//! The empty `alloc` feature is retained for compatibility, not alloc-only support.
//! Full `no_std` + `alloc` support is deferred; see
//! [issue #646](https://github.com/FreeTAKTeam/LXMF-rs/issues/646).

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
