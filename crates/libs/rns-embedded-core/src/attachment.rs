use crate::{EmbeddedError, EmbeddedResult};
use alloc::vec::Vec;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChunkCursor {
    pub transfer_id: u32,
    pub next_offset: u32,
    pub chunk_size: u16,
}

impl ChunkCursor {
    pub fn validate(&self) -> EmbeddedResult<()> {
        if self.transfer_id == 0 || self.chunk_size == 0 {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttachmentChunk {
    pub transfer_id: u32,
    pub sequence: u16,
    pub payload: Vec<u8>,
}
