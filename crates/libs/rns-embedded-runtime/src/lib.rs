#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod ble;

pub mod constants;

pub mod node;

#[cfg(feature = "std")]
pub mod tcp;

include!("lib_parts/module_prelude.rs");

include!("lib_parts/config.rs");
