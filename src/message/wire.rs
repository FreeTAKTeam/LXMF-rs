use crate::error::LxmfError;
use crate::message::Payload;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct WireMessage {
    pub destination: [u8; 16],
    pub source: [u8; 16],
    pub payload: Payload,
}

impl WireMessage {
    pub fn new(destination: [u8; 16], source: [u8; 16], payload: Payload) -> Self {
        Self {
            destination,
            source,
            payload,
        }
    }

    pub fn message_id(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.destination);
        hasher.update(self.source);
        hasher.update(self.payload.to_msgpack().unwrap_or_default());
        let bytes = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }

    pub fn pack(&self) -> Result<Vec<u8>, LxmfError> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.destination);
        out.extend_from_slice(&self.source);
        let payload = self
            .payload
            .to_msgpack()
            .map_err(|_| LxmfError::Unimplemented)?;
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn unpack(bytes: &[u8]) -> Result<Self, LxmfError> {
        if bytes.len() < 32 {
            return Err(LxmfError::Unimplemented);
        }
        let mut dest = [0u8; 16];
        let mut src = [0u8; 16];
        dest.copy_from_slice(&bytes[0..16]);
        src.copy_from_slice(&bytes[16..32]);
        let payload = Payload::from_msgpack(&bytes[32..]).map_err(|_| LxmfError::Unimplemented)?;
        Ok(Self::new(dest, src, payload))
    }
}
