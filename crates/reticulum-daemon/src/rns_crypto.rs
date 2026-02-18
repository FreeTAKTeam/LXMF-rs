use rand_core::CryptoRngCore;
use reticulum::error::RnsError;
use reticulum::identity::PrivateIdentity;
use x25519_dalek::{PublicKey, StaticSecret};

pub fn encrypt_for_public_key<R: CryptoRngCore + Copy>(
    public_key: &PublicKey,
    salt: &[u8],
    plaintext: &[u8],
    rng: R,
) -> Result<Vec<u8>, RnsError> {
    reticulum::ratchets::encrypt_for_public_key(public_key, salt, plaintext, rng)
}

pub fn decrypt_with_private_key(
    private_key: &StaticSecret,
    salt: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RnsError> {
    reticulum::ratchets::decrypt_with_private_key(private_key, salt, ciphertext)
}

pub fn decrypt_with_identity(
    identity: &PrivateIdentity,
    salt: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RnsError> {
    reticulum::ratchets::decrypt_with_identity(identity, salt, ciphertext)
}
