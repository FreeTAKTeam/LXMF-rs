use crate::{EmbeddedError, EmbeddedResult};
use alloc::vec::Vec;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PacketFrame {
    pub kind: u8,
    pub sequence: u32,
    pub payload: Vec<u8>,
}

impl PacketFrame {
    pub fn new(kind: u8, sequence: u32, payload: Vec<u8>) -> EmbeddedResult<Self> {
        if payload.is_empty() {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(Self { kind, sequence, payload })
    }
}
