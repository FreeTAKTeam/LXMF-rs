use crate::{EmbeddedError, EmbeddedResult};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EmbeddedIdentity {
    pub id: [u8; 32],
    pub verify_key: [u8; 32],
}

impl EmbeddedIdentity {
    pub fn new(id: [u8; 32], verify_key: [u8; 32]) -> EmbeddedResult<Self> {
        if id == [0; 32] || verify_key == [0; 32] {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(Self { id, verify_key })
    }
}

pub trait IdentityProvider {
    fn active_identity(&self) -> EmbeddedResult<EmbeddedIdentity>;
}
