//! In-process [`lxmf_sdk::SdkBackend`] implementation over Reticulum transport.

// The public backend trait fixes `SdkError` as the error type, so boxing it in
// this adapter would make the implementation incompatible with the SDK.
#![allow(clippy::result_large_err)]

mod backend;
mod config;
mod delivery;
mod link_delivery;
mod state;
#[cfg(test)]
mod tests;

pub use backend::InProcessBackend;
pub use config::{InProcessBackendConfig, InProcessBackendLimits};
pub use delivery::{DeliveryMethod, DeliveryOutcome, DeliveryRepresentation, InProcessSendReport};

pub const EXT_ACCEPTED_RESULT_ACK: &str = "reticulum.accepted_result_ack";
pub const EXT_DIRECT_PACKET_MAX_WIRE_BYTES: &str = "reticulum.direct_packet_max_wire_bytes";
pub const EXT_FIELDS_BASE64: &str = "reticulum.fields_base64";
pub const EXT_LINK_CONNECT_TIMEOUT_MS: &str = "reticulum.link_connect_timeout_ms";
pub const EXT_PROPAGATION_RELAY_HEX: &str = "reticulum.propagation_relay_hex";
pub const EXT_RAW_BYTES_BASE64: &str = "reticulum.raw_bytes_base64";
pub const EXT_SEND_MODE: &str = "reticulum.send_mode";
pub const EXT_USE_PROPAGATION_NODE: &str = "reticulum.use_propagation_node";
