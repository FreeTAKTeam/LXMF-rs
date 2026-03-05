use crate::{EmbeddedError, EmbeddedResult};
use alloc::{string::String, vec::Vec};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MinimalEnvelope {
    pub source: [u8; 16],
    pub destination: [u8; 16],
    pub body: Vec<u8>,
}

impl MinimalEnvelope {
    pub fn validate(&self) -> EmbeddedResult<()> {
        if self.source == [0; 16] || self.destination == [0; 16] {
            return Err(EmbeddedError::InvalidInput);
        }
        if self.body.is_empty() {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        alloc::format!(
            "src={:02x?} dst={:02x?} bytes={}",
            &self.source[..4],
            &self.destination[..4],
            self.body.len()
        )
    }
}
