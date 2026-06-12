#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod ble;

pub mod constants;

pub mod node;

#[cfg(feature = "std")]
pub mod tcp;

include!("lib_parts/part_001_part_001.rs");

include!("lib_parts/part_002_config.rs");
