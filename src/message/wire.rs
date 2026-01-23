use crate::error::LxmfError;
use crate::message::Payload;
use ed25519_dalek::Signature;
use reticulum::identity::{Identity, PrivateIdentity};
use sha2::{Digest, Sha256};

pub const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;

#[derive(Debug, Clone)]
pub struct WireMessage {
    pub destination: [u8; 16],
    pub source: [u8; 16],
    pub signature: Option<[u8; SIGNATURE_LENGTH]>,
    pub payload: Payload,
}

impl WireMessage {
    pub fn new(destination: [u8; 16], source: [u8; 16], payload: Payload) -> Self {
        Self {
            destination,
            source,
            signature: None,
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

    pub fn sign(&mut self, signer: &PrivateIdentity) -> Result<(), LxmfError> {
        let payload = self.payload.to_msgpack()?;
        let mut data = Vec::with_capacity(16 + 16 + payload.len() + 32);
        data.extend_from_slice(&self.destination);
        data.extend_from_slice(&self.source);
        data.extend_from_slice(&payload);
        data.extend_from_slice(&self.message_id());

        let signature = signer.sign(&data);
        self.signature = Some(signature.to_bytes());
        Ok(())
    }

    pub fn verify(&self, identity: &Identity) -> Result<bool, LxmfError> {
        let Some(sig_bytes) = self.signature else {
            return Ok(false);
        };
        let signature = Signature::from_slice(&sig_bytes)
            .map_err(|e| LxmfError::Decode(e.to_string()))?;

        let payload = self.payload.to_msgpack()?;
        let mut data = Vec::with_capacity(16 + 16 + payload.len() + 32);
        data.extend_from_slice(&self.destination);
        data.extend_from_slice(&self.source);
        data.extend_from_slice(&payload);
        data.extend_from_slice(&self.message_id());

        Ok(identity.verify(&data, &signature).is_ok())
    }

    pub fn pack(&self) -> Result<Vec<u8>, LxmfError> {
        let signature = self
            .signature
            .ok_or_else(|| LxmfError::Encode("missing signature".into()))?;
        let mut out = Vec::new();
        out.extend_from_slice(&self.destination);
        out.extend_from_slice(&self.source);
        out.extend_from_slice(&signature);
        let payload = self.payload.to_msgpack()?;
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn unpack(bytes: &[u8]) -> Result<Self, LxmfError> {
        let min_len = 16 + 16 + SIGNATURE_LENGTH;
        if bytes.len() < min_len {
            return Err(LxmfError::Decode("wire message too short".into()));
        }
        let mut dest = [0u8; 16];
        let mut src = [0u8; 16];
        let mut signature = [0u8; SIGNATURE_LENGTH];
        dest.copy_from_slice(&bytes[0..16]);
        src.copy_from_slice(&bytes[16..32]);
        signature.copy_from_slice(&bytes[32..32 + SIGNATURE_LENGTH]);
        let payload = Payload::from_msgpack(&bytes[32 + SIGNATURE_LENGTH..])?;
        Ok(Self {
            destination: dest,
            source: src,
            signature: Some(signature),
            payload,
        })
    }
}
