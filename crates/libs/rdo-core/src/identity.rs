//! RNS Identity and Destination arithmetic without the transport stack:
//! hashing, SINGLE Destination derivation and Ed25519.

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest as _, Sha256};

use crate::error::{RdoError, Result};

pub type Destination = [u8; 16];
pub type Digest = [u8; 32];
pub type Signature = [u8; 64];

pub fn sha256(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}

/// `SHA256(name_hash || identity_hash)[0:16]`, the RNS SINGLE Destination.
pub fn destination_hash(public_key: &[u8; 64], full_name: &str) -> Destination {
    let name_hash = sha256(full_name.as_bytes());
    let identity_hash = sha256(public_key);
    let mut material = [0u8; 26];
    material[..10].copy_from_slice(&name_hash[..10]);
    material[10..].copy_from_slice(&identity_hash[..16]);
    truncate(&sha256(&material))
}

pub fn truncate(digest: &Digest) -> Destination {
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

/// A public RNS Identity: X25519 public key followed by Ed25519 verifying key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicIdentity(pub [u8; 64]);

impl PublicIdentity {
    /// RNS identity hash, `SHA256(public_key)[0:16]`.
    pub fn hash(&self) -> Destination {
        truncate(&sha256(&self.0))
    }

    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        let mut key = [0u8; 32];
        key.copy_from_slice(&self.0[32..]);
        let key = VerifyingKey::from_bytes(&key).map_err(|_| RdoError::InvalidSignature)?;
        key.verify(message, &ed25519_dalek::Signature::from_bytes(signature))
            .map_err(|_| RdoError::InvalidSignature)
    }
}

/// The signing half of an RNS Identity (private X25519 key ‖ Ed25519 seed).
pub struct SigningIdentity {
    signing: SigningKey,
    public: PublicIdentity,
}

impl SigningIdentity {
    pub fn from_private_bytes(private: &[u8; 64]) -> Self {
        let mut x25519 = [0u8; 32];
        x25519.copy_from_slice(&private[..32]);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&private[32..]);
        let signing = SigningKey::from_bytes(&seed);
        let x25519_public = x25519_public_key(&x25519);
        let mut public = [0u8; 64];
        public[..32].copy_from_slice(&x25519_public);
        public[32..].copy_from_slice(signing.verifying_key().as_bytes());
        Self { signing, public: PublicIdentity(public) }
    }

    pub fn public(&self) -> PublicIdentity {
        self.public
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing.sign(message).to_bytes()
    }
}

fn x25519_public_key(private: &[u8; 32]) -> [u8; 32] {
    let secret = x25519_dalek::StaticSecret::from(*private);
    *x25519_dalek::PublicKey::from(&secret).as_bytes()
}
