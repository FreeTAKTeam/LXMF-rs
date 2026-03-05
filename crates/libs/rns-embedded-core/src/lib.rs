#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod attachment;
pub mod identity;
pub mod lxmf_min;
pub mod packet;
pub mod store;
pub mod transport;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EmbeddedError {
    InvalidInput,
    InvalidState,
    IntegrityFailure,
    ReplayRejected,
    Timeout,
    Backpressure,
    Disconnected,
    StorageCorruption,
    Unsupported,
}

pub type EmbeddedResult<T> = core::result::Result<T, EmbeddedError>;
