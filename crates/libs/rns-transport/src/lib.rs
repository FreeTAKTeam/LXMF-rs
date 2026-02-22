//! Transport boundary crate for runtime crates and daemon entrypoints.

#![allow(clippy::unwrap_used)]

extern crate alloc;

pub mod buffer;
pub mod channel;
pub mod config;
pub mod crypt;
pub mod delivery;
pub mod destination;
pub mod destination_hash;
pub mod embedded_link;
pub mod error;
pub mod hash;
pub mod identity;
pub mod iface;
pub mod packet;
pub mod ratchets;
pub mod receipt;
pub mod resource;
pub mod serde;
pub mod storage;
pub mod time;
pub mod transport;
pub mod utils;

pub mod identity_bridge;

pub use packet::{DestinationType, Packet, PacketContext, PacketDataBuffer, PacketType};
