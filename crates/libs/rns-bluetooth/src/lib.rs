//! Protocol-neutral Bluetooth I/O. Platform adapters own native resources;
//! callers own discovery policy, framing, reconnects and authorization.
mod error;
mod lifecycle;
mod transport;
mod types;

pub use error::{BluetoothError, ErrorKind, Result};
pub use lifecycle::{EventQueue, SessionRegistry};
pub use transport::{ClassicStream, GattClient, GattServer, ServerRequest, ServerRequestKind};
pub use types::*;

#[cfg(feature = "android")]
pub mod android;
