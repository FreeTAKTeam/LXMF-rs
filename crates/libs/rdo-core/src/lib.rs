//! Reticulum Distributed Object (RDO) shared-Identity owner binding v1.
//!
//! Transport-independent: derives object Destinations, binds signed
//! announces to the requested object, signs and verifies immutable
//! revisions, and serves owner operations with single-owner commits and
//! replay. The optional `rns` feature validates raw RNS announce packets.
//! RDO does not depend on RPP or LXMF.

pub mod cbor;
pub mod error;
pub mod identity;
pub mod lookup;
pub mod owner;
pub mod revision;
#[cfg(feature = "rns")]
pub mod rns;

pub use error::{RdoError, Result};
pub use identity::{Destination, Digest, PublicIdentity, SigningIdentity};
pub use lookup::{object_destination, ObjectBinding, Token};
pub use owner::{Op, OpResult, Owner, Policy, Request, Status};
pub use revision::Revision;
