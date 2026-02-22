use crate::{
    crypt::fernet::{Fernet, PlainText, Token},
    error::RnsError,
    packet::DestinationType,
};
use alloc::vec;
use alloc::vec::Vec;
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

pub fn group_encrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, RnsError> {
    let fernet = Fernet::new_from_slices(key, key, OsRng);
    let mut out_buf = vec![0u8; data.len() + 64];
    let token = fernet.encrypt(PlainText::from(data), &mut out_buf)?;
    Ok(token.as_bytes().to_vec())
}

pub fn group_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, RnsError> {
    let fernet = Fernet::new_from_slices(key, key, OsRng);
    let token = Token::from(data);
    let verified = fernet.verify(token)?;
    let mut out_buf = vec![0u8; data.len()];
    let plaintext = fernet.decrypt(verified, &mut out_buf)?;
    Ok(plaintext.as_bytes().to_vec())
}
