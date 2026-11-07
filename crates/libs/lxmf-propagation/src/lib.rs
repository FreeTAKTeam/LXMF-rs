//! Propagation storage shared by the daemon and embedded runtimes.
//! No transport, RPC dispatcher or process lifecycle is imported here.
mod error;
pub mod identities;
#[cfg(feature = "embedded")]
mod owned_identities;
#[cfg(feature = "embedded")]
mod owned_schema;
mod owned_types;
#[cfg(feature = "embedded")]
pub mod prepared;
pub mod protocol;
pub mod storage;
pub mod validation;
pub use error::Error;
pub use owned_types::*;
#[cfg(feature = "embedded")]
pub use prepared::PreparedMessage;
#[cfg(feature = "embedded")]
mod owned;
#[cfg(feature = "embedded")]
mod owned_changes;
#[cfg(feature = "embedded")]
mod owned_recovery;
#[cfg(feature = "embedded")]
pub use owned::OwnedStore;
#[cfg(all(test, feature = "embedded"))]
mod tests;

pub mod handlers;
