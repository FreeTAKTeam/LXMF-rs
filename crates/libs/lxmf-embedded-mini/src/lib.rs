#![no_std]

pub mod error;
pub mod runtime;
pub mod store;
pub mod transport;
pub mod wire;

pub use error::{MiniError, MiniResult};
pub use runtime::{MiniEvent, MiniNodeRuntime, MiniRuntimeConfig, MiniRuntimeStats, RuntimeStatus};
pub use store::{MiniStore, NullStore, RamReplayStore};
pub use transport::{LinkState, MiniTransport};
pub use wire::{MiniMessage, HASH_LENGTH, SIGNATURE_LENGTH, WIRE_HEADER_LENGTH};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
