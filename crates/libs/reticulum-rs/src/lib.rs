//! Umbrella crate for Reticulum primitives, transport, and daemon RPC contracts.

#[cfg(feature = "core")]
pub use rns_core as core;
#[cfg(feature = "rpc")]
pub use rns_rpc as rpc;
#[cfg(feature = "transport")]
pub use rns_transport as transport;

#[cfg(feature = "core")]
pub use rns_core::{destination, hash, identity, packet, ratchets};
#[cfg(feature = "transport")]
pub use rns_transport::{iface, resource, transport as runtime};
