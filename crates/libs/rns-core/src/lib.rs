//! Core Reticulum packet, identity, ratchet, and cryptographic primitives.
//!
//! # `no_std` support
//!
//! With `--no-default-features --features alloc` this crate builds for
//! bare-metal targets; `tools/scripts/check-no-std-core.sh` verifies that on
//! `thumbv7em-none-eabi` in CI. In that configuration the embedding
//! application must supply a clock through [`ratchets::set_time_override`]
//! and pass its own [`rand_core::CryptoRngCore`] where an RNG is required
//! (for example [`group_encrypt_with_rng`]). The `std` feature adds the OS
//! RNG, [`group_encrypt`], the filesystem key store, and ratchet persistence.
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

#[cfg(feature = "std")]
pub mod secure_storage;
pub mod serde;

#[cfg(feature = "std")]
pub use destination::group_encrypt;
pub use destination::{group_decrypt, group_encrypt_with_rng};
pub use error::RnsError;
pub use hash::lxmf_address_hash;
pub use identity::{lxmf_sign, lxmf_verify};
pub use packet::{Packet, LXMF_MAX_PAYLOAD};
