use crate::error::LxmfError;
use crate::message::Payload;
use ed25519_dalek::Signature;
use reticulum::identity::{Identity, PrivateIdentity};
use sha2::{Digest, Sha256};

pub const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;
const STORAGE_MAGIC: &[u8; 8] = b"LXMFSTR0";
const STORAGE_VERSION: u8 = 1;
const STORAGE_FLAG_HAS_SIGNATURE: u8 = 0x01;

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

    pub fn pack_storage(&self) -> Result<Vec<u8>, LxmfError> {
        let payload = self.payload.to_msgpack()?;
        let mut out = Vec::with_capacity(
            STORAGE_MAGIC.len()
                + 1
                + 1
                + 16
                + 16
                + self.signature.map(|_| SIGNATURE_LENGTH).unwrap_or(0)
                + payload.len(),
        );
        out.extend_from_slice(STORAGE_MAGIC);
        out.push(STORAGE_VERSION);
        let mut flags = 0u8;
        if self.signature.is_some() {
            flags |= STORAGE_FLAG_HAS_SIGNATURE;
        }
        out.push(flags);
        out.extend_from_slice(&self.destination);
        out.extend_from_slice(&self.source);
        if let Some(signature) = self.signature {
            out.extend_from_slice(&signature);
        }
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

    pub fn unpack_storage(bytes: &[u8]) -> Result<Self, LxmfError> {
        let magic_len = STORAGE_MAGIC.len();
        if bytes.len() >= magic_len && bytes.starts_with(STORAGE_MAGIC) {
            if bytes.len() < magic_len + 1 + 1 + 16 + 16 {
                return Err(LxmfError::Decode("storage message too short".into()));
            }
            let version = bytes[magic_len];
            if version != STORAGE_VERSION {
                return Err(LxmfError::Decode("unsupported storage version".into()));
            }
            let flags = bytes[magic_len + 1];
            let mut idx = magic_len + 2;
            let mut dest = [0u8; 16];
            let mut src = [0u8; 16];
            dest.copy_from_slice(&bytes[idx..idx + 16]);
            idx += 16;
            src.copy_from_slice(&bytes[idx..idx + 16]);
            idx += 16;
            let signature = if flags & STORAGE_FLAG_HAS_SIGNATURE != 0 {
                if bytes.len() < idx + SIGNATURE_LENGTH {
                    return Err(LxmfError::Decode("storage signature missing".into()));
                }
                let mut sig = [0u8; SIGNATURE_LENGTH];
                sig.copy_from_slice(&bytes[idx..idx + SIGNATURE_LENGTH]);
                idx += SIGNATURE_LENGTH;
                Some(sig)
            } else {
                None
            };
            let payload = Payload::from_msgpack(&bytes[idx..])?;
            return Ok(Self {
                destination: dest,
                source: src,
                signature,
                payload,
            });
        }

        Self::unpack(bytes)
    }
}
