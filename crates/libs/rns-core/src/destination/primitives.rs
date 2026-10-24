use crate::{
    crypt::fernet::{CachedFernet, PlainText, Token},
    error::RnsError,
    packet::DestinationType,
};
use alloc::vec;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use rand_core::OsRng;

pub trait Direction {}

pub struct Input;
pub struct Output;

impl Direction for Input {}
impl Direction for Output {}

pub trait Type {
    fn destination_type() -> DestinationType;
}

pub struct Single;
pub struct Plain;
pub struct Group;

impl Type for Single {
    fn destination_type() -> DestinationType {
        DestinationType::Single
    }
}

impl Type for Plain {
    fn destination_type() -> DestinationType {
        DestinationType::Plain
    }
}

impl Type for Group {
    fn destination_type() -> DestinationType {
        DestinationType::Group
    }
}

/// Encrypts `data` for a group destination using the OS random source.
///
/// Requires the `std` feature; `no_std` callers use [`group_encrypt_with_rng`].
#[cfg(feature = "std")]
pub fn group_encrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, RnsError> {
    group_encrypt_with_rng(OsRng, key, data)
}

/// Encrypts `data` for a group destination with a caller-supplied RNG.
///
/// Available in both `std` and `no_std` builds; the RNG seeds the Fernet IV.
pub fn group_encrypt_with_rng<R: rand_core::CryptoRngCore>(
    rng: R,
    key: &[u8; 16],
    data: &[u8],
) -> Result<Vec<u8>, RnsError> {
    let fernet = CachedFernet::new_from_slices(key, key);
    let mut out_buf = vec![0u8; data.len() + 64];
    let token = fernet.encrypt(rng, PlainText::from(data), &mut out_buf)?;
    Ok(token.as_bytes().to_vec())
}

pub fn group_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, RnsError> {
    let fernet = CachedFernet::new_from_slices(key, key);
    let token = Token::from(data);
    let verified = fernet.verify(token)?;
    let mut out_buf = vec![0u8; data.len()];
    let plaintext = fernet.decrypt(verified, &mut out_buf)?;
    Ok(plaintext.as_bytes().to_vec())
}
