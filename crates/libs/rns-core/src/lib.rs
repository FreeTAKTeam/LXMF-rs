#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod buffer;
pub mod crypt;
pub mod destination;
pub mod destination_hash;
mod error;
pub mod hash;
pub mod identity;
pub mod key_manager;
pub mod packet;
pub mod ratchets;

pub mod serde;

pub use destination::{group_decrypt, group_encrypt};
pub use error::RnsError;
pub use hash::lxmf_address_hash;
pub use identity::{lxmf_sign, lxmf_verify};
pub use packet::{Packet, LXMF_MAX_PAYLOAD};
