use crate::error::LxmfError;
use crate::message::Payload;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use base64::Engine;
use ed25519_dalek::Signature;
use rand_core::CryptoRngCore;
use rns_core::crypt::fernet::{Fernet, PlainText, FERNET_MAX_PADDING_SIZE, FERNET_OVERHEAD_SIZE};
use rns_core::identity::{DerivedKey, Identity, PrivateIdentity, PUBLIC_KEY_LENGTH};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey};

pub const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;
pub const LXM_URI_PREFIX: &str = "lxm://";
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
        Self { destination, source, signature: None, payload }
    }

    pub fn message_id(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.destination);
        hasher.update(self.source);
        hasher.update(self.payload.to_msgpack_without_stamp().unwrap_or_default());
        let bytes = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }

    pub fn sign(&mut self, signer: &PrivateIdentity) -> Result<(), LxmfError> {
        let payload = self.payload.to_msgpack_without_stamp()?;
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
            .map_err(|e: ed25519_dalek::SignatureError| LxmfError::Decode(e.to_string()))?;

        let payload = self.payload.to_msgpack_without_stamp()?;
        let mut data = Vec::with_capacity(16 + 16 + payload.len() + 32);
        data.extend_from_slice(&self.destination);
        data.extend_from_slice(&self.source);
        data.extend_from_slice(&payload);
        data.extend_from_slice(&self.message_id());

        Ok(identity.verify(&data, &signature).is_ok())
    }

    pub fn pack(&self) -> Result<Vec<u8>, LxmfError> {
        let signature =
            self.signature.ok_or_else(|| LxmfError::Encode("missing signature".into()))?;
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
        Ok(Self { destination: dest, source: src, signature: Some(signature), payload })
    }

    #[cfg(feature = "std")]
    pub fn unpack_from_file(path: impl AsRef<std::path::Path>) -> Result<Self, LxmfError> {
        let bytes = std::fs::read(path).map_err(|e| LxmfError::Io(e.to_string()))?;
        Self::unpack(&bytes)
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
            return Ok(Self { destination: dest, source: src, signature, payload });
        }

        Self::unpack(bytes)
    }

    #[cfg(feature = "std")]
    pub fn unpack_storage_from_file(path: impl AsRef<std::path::Path>) -> Result<Self, LxmfError> {
        let bytes = std::fs::read(path).map_err(|e| LxmfError::Io(e.to_string()))?;
        Self::unpack_storage(&bytes)
    }

    #[cfg(feature = "std")]
    pub fn pack_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<(), LxmfError> {
        let bytes = self.pack()?;
        std::fs::write(path, bytes).map_err(|e| LxmfError::Io(e.to_string()))
    }

    #[cfg(feature = "std")]
    pub fn pack_storage_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<(), LxmfError> {
        let bytes = self.pack_storage()?;
        std::fs::write(path, bytes).map_err(|e| LxmfError::Io(e.to_string()))
    }

    pub fn pack_propagation_with_rng<R: CryptoRngCore + Copy>(
        &self,
        destination: &Identity,
        timestamp: f64,
        rng: R,
    ) -> Result<Vec<u8>, LxmfError> {
        let (envelope, _) =
            self.pack_propagation_with_options_and_rng(destination, timestamp, None, rng)?;
        Ok(envelope)
    }

    pub fn pack_propagation_with_options_and_rng<R: CryptoRngCore + Copy>(
        &self,
        destination: &Identity,
        timestamp: f64,
        propagation_stamp: Option<&[u8]>,
        rng: R,
    ) -> Result<(Vec<u8>, [u8; 32]), LxmfError> {
        let packed = self.pack()?;
        let encrypted = encrypt_for_identity(destination, &packed[16..], rng)?;

        let mut lxmf_data = Vec::with_capacity(16 + encrypted.len());
        lxmf_data.extend_from_slice(&packed[..16]);
        lxmf_data.extend_from_slice(&encrypted);
        let transient_id = Sha256::digest(&lxmf_data);
        let mut transient_payload = lxmf_data;
        if let Some(stamp) = propagation_stamp {
            transient_payload.extend_from_slice(stamp);
        }

        let envelope = (timestamp, vec![serde_bytes::ByteBuf::from(transient_payload)]);
        let packed =
            rmp_serde::to_vec(&envelope).map_err(|e| LxmfError::Encode(e.to_string()))?;
        let mut transient_id_bytes = [0u8; 32];
        transient_id_bytes.copy_from_slice(transient_id.as_slice());
        Ok((packed, transient_id_bytes))
    }

    pub fn pack_paper_with_rng<R: CryptoRngCore + Copy>(
        &self,
        destination: &Identity,
        rng: R,
    ) -> Result<Vec<u8>, LxmfError> {
        let packed = self.pack()?;
        let encrypted = encrypt_for_identity(destination, &packed[16..], rng)?;
        let mut out = Vec::with_capacity(16 + encrypted.len());
        out.extend_from_slice(&packed[..16]);
        out.extend_from_slice(&encrypted);
        Ok(out)
    }

    pub fn pack_paper_uri_with_rng<R: CryptoRngCore + Copy>(
        &self,
        destination: &Identity,
        rng: R,
    ) -> Result<String, LxmfError> {
        let packed = self.pack_paper_with_rng(destination, rng)?;
        Ok(Self::encode_lxm_uri(&packed))
    }

    pub fn encode_lxm_uri(paper_bytes: &[u8]) -> String {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(paper_bytes);
        format!("{LXM_URI_PREFIX}{encoded}")
    }

    pub fn decode_lxm_uri(uri: &str) -> Result<Vec<u8>, LxmfError> {
        let encoded = uri
            .strip_prefix(LXM_URI_PREFIX)
            .ok_or_else(|| LxmfError::Decode("invalid lxm uri prefix".into()))?;

        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
            .map_err(|e| LxmfError::Decode(format!("invalid lxm uri payload: {e}")))
    }
}

fn encrypt_for_identity<R: CryptoRngCore + Copy>(
    destination: &Identity,
    plaintext: &[u8],
    rng: R,
) -> Result<Vec<u8>, LxmfError> {
    let secret = EphemeralSecret::random_from_rng(rng);
    let ephemeral_public = PublicKey::from(&secret);
    let shared = secret.diffie_hellman(&destination.public_key);
    let derived = DerivedKey::new(&shared, Some(destination.address_hash.as_slice()));
    let key_bytes = derived.as_bytes();
    let split = key_bytes.len() / 2;

    let fernet = Fernet::new_from_slices(&key_bytes[..split], &key_bytes[split..], rng);
    // Use shared Fernet bounds from reticulum-rs to avoid token sizing drift.
    let token_capacity = plaintext.len() + FERNET_OVERHEAD_SIZE + FERNET_MAX_PADDING_SIZE;
    let mut out = vec![0u8; PUBLIC_KEY_LENGTH + token_capacity];
    out[..PUBLIC_KEY_LENGTH].copy_from_slice(ephemeral_public.as_bytes());
    let token = fernet
        .encrypt(PlainText::from(plaintext), &mut out[PUBLIC_KEY_LENGTH..])
        .map_err(|e| LxmfError::Encode(format!("{e:?}")))?;
    let total = PUBLIC_KEY_LENGTH + token.len();
    out.truncate(total);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::WireMessage;
    use crate::message::Payload;
    use rand_core::OsRng;
    use rns_core::identity::PrivateIdentity;
    use serde_bytes::ByteBuf;
    use sha2::{Digest, Sha256};

    fn address_hash_bytes(identity: &PrivateIdentity) -> [u8; 16] {
        let mut out = [0u8; 16];
        out.copy_from_slice(identity.address_hash().as_slice());
        out
    }

    #[test]
    fn propagation_pack_derives_transient_id_from_lxm_data() {
        let sender = PrivateIdentity::new_from_name("propagation-pack-sender");
        let receiver = PrivateIdentity::new_from_name("propagation-pack-receiver");
        let payload =
            Payload::new(1.0, Some(b"content".to_vec()), Some(b"title".to_vec()), None, None);
        let mut wire =
            WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
        wire.sign(&sender).expect("sign");

        let (envelope, transient_id) = wire
            .pack_propagation_with_options_and_rng(receiver.as_identity(), 2.0, None, OsRng)
            .expect("pack propagation");
        let (_timestamp, entries): (f64, Vec<ByteBuf>) =
            rmp_serde::from_slice(&envelope).expect("decode propagation envelope");
        assert_eq!(entries.len(), 1);

        let expected = Sha256::digest(entries[0].as_ref());
        assert_eq!(transient_id.as_slice(), expected.as_slice());
    }

    #[test]
    fn propagation_pack_appends_optional_stamp_after_lxm_data() {
        let sender = PrivateIdentity::new_from_name("propagation-pack-stamp-sender");
        let receiver = PrivateIdentity::new_from_name("propagation-pack-stamp-receiver");
        let payload = Payload::new(1.0, Some(vec![0x55; 48]), Some(b"title".to_vec()), None, None);
        let mut wire =
            WireMessage::new(address_hash_bytes(&receiver), address_hash_bytes(&sender), payload);
        wire.sign(&sender).expect("sign");

        let propagation_stamp = vec![0xAB; 32];
        let (envelope, transient_id) = wire
            .pack_propagation_with_options_and_rng(
                receiver.as_identity(),
                3.0,
                Some(propagation_stamp.as_slice()),
                OsRng,
            )
            .expect("pack propagation with stamp");
        let (_timestamp, entries): (f64, Vec<ByteBuf>) =
            rmp_serde::from_slice(&envelope).expect("decode propagation envelope");
        let transient_payload = entries[0].as_ref();

        assert!(transient_payload.ends_with(propagation_stamp.as_slice()));
        let lxm_data = &transient_payload[..transient_payload.len() - propagation_stamp.len()];
        let expected = Sha256::digest(lxm_data);
        assert_eq!(transient_id.as_slice(), expected.as_slice());
    }
}
