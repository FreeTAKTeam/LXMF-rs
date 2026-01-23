use ed25519_dalek::Signature;
use reticulum::hash::AddressHash;
use reticulum::identity::{Identity, PrivateIdentity};

pub struct Adapter;

impl Adapter {
    pub const DEST_HASH_LEN: usize = 16;

    pub fn new() -> Self {
        Self
    }

    pub fn address_hash(identity: &Identity) -> [u8; Self::DEST_HASH_LEN] {
        let mut out = [0u8; Self::DEST_HASH_LEN];
        out.copy_from_slice(identity.address_hash.as_slice());
        out
    }

    pub fn address_hash_from_dest(dest: &AddressHash) -> [u8; Self::DEST_HASH_LEN] {
        let mut out = [0u8; Self::DEST_HASH_LEN];
        out.copy_from_slice(dest.as_slice());
        out
    }

    pub fn sign(identity: &PrivateIdentity, data: &[u8]) -> [u8; ed25519_dalek::SIGNATURE_LENGTH] {
        identity.sign(data).to_bytes()
    }

    pub fn verify(identity: &Identity, data: &[u8], signature: &[u8]) -> bool {
        let signature = match Signature::from_slice(signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        identity.verify(data, &signature).is_ok()
    }
}
