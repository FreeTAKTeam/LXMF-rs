//! Identity conversion helpers for explicit boundary crossing.

use crate::identity;
use rns_core::identity::{Identity as CoreIdentity, PrivateIdentity as CorePrivateIdentity};

/// Convert a core identity into a transport identity.
///
/// This is intentionally explicit so boundary usage stays clear at call sites.
pub fn to_transport_identity(identity: &CoreIdentity) -> identity::Identity {
    identity::Identity::new_from_slices(identity.public_key_bytes(), identity.verifying_key_bytes())
}

/// Convert a core private identity into a transport private identity.
///
/// Identity bytes are copied through the canonical byte format and validated.
pub fn to_transport_private_identity(private: &CorePrivateIdentity) -> identity::PrivateIdentity {
    identity::PrivateIdentity::from_private_key_bytes(&private.to_private_key_bytes())
        .expect("core private identity bytes are always valid")
}

/// Convert a transport identity into a core identity.
pub fn to_core_identity(identity: &identity::Identity) -> CoreIdentity {
    CoreIdentity::new_from_slices(identity.public_key_bytes(), identity.verifying_key_bytes())
}

/// Convert a transport private identity into a core private identity.
pub fn to_core_private_identity(private: &identity::PrivateIdentity) -> CorePrivateIdentity {
    CorePrivateIdentity::from_private_key_bytes(&private.to_private_key_bytes())
        .expect("transport private identity bytes are always valid")
}
