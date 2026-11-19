//! Reticulum Package Protocol (RPP 0.3 RC1) for Rust.
//!
//! `rpp-core` is transport-independent ("sans-IO"): it encodes and verifies
//! RPP bytes, keeps replay and transfer state, and tells its caller which
//! RNS Link requests and Resources to send. It has no daemon, LXMF, RCH or
//! REM dependency. The optional `rns` feature adds an async adapter onto
//! `reticulum-rs-transport`. See `docs/rpp/rpp-core.md`.

pub mod cbor;
pub mod control;
pub mod error;
pub mod identity;
pub mod inbox;
pub mod manifest;
pub mod manifest_build;
pub mod publication;
pub mod publisher;
pub mod receiver;
pub mod replay;
#[cfg(feature = "rns")]
pub mod rns;
pub mod selection;
pub mod store;

pub use control::{Body, Control, Header, RppResult, Status};
pub use error::{Result, RppError};
pub use identity::{Destination, Digest, PublicIdentity, SigningIdentity};
pub use manifest::{OwnerBinding, VerifiedManifest};
pub use manifest_build::{FileDraft, ManifestDraft};
pub use publisher::{Outbound, Publisher, Served};
pub use receiver::{Progress, Pull, PullConfig};

/// RNS Link request path selecting wire version 3.
pub const REQUEST_PATH: &str = "/rpp/3";

/// Native RNS Resource metadata for an RPP Resource: the 16-byte ID packed
/// as one msgpack bin value (`c4 10 ‖ id`), as `RNS.Resource` does.
pub fn pack_resource_metadata(id: &control::RequestId) -> Vec<u8> {
    [&[0xc4, 0x10][..], id].concat()
}

pub fn unpack_resource_metadata(metadata: &[u8]) -> Result<control::RequestId> {
    match metadata {
        [0xc4, 0x10, id @ ..] => id.try_into().map_err(|_| RppError::NoAcceptedPlan),
        _ => Err(RppError::NoAcceptedPlan),
    }
}
