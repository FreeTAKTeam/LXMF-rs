use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rand_core::CryptoRngCore;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

use crate::crypt::fernet::{
    Fernet, PlainText, Token, FERNET_MAX_PADDING_SIZE, FERNET_OVERHEAD_SIZE,
};
use crate::error::RnsError;
use crate::hash::AddressHash;
use crate::identity::{DerivedKey, PrivateIdentity, PUBLIC_KEY_LENGTH};

const RATCHET_EXPIRY_SECS: f64 = 30.0 * 24.0 * 60.0 * 60.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RatchetRecord {
    pub ratchet: ByteBuf,
    pub received: f64,
}

#[derive(Debug)]
pub(crate) struct RatchetStore {
    ratchet_dir: PathBuf,
    cache: HashMap<AddressHash, RatchetRecord>,
}

impl RatchetStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { ratchet_dir: path, cache: HashMap::new() }
    }

    pub(crate) fn remember(
        &mut self,
        destination: &AddressHash,
        ratchet: [u8; PUBLIC_KEY_LENGTH],
    ) -> Result<(), RnsError> {
        let now = now_secs();
        if let Some(existing) = self.cache.get(destination) {
            if existing.ratchet.as_ref() == ratchet.as_slice() {
                return Ok(());
            }
        }

        let record = RatchetRecord { ratchet: ByteBuf::from(ratchet.to_vec()), received: now };
        self.cache.insert(*destination, record.clone());
        self.persist_record(destination, &record)?;
        Ok(())
    }

    pub(crate) fn get(&mut self, destination: &AddressHash) -> Option<[u8; PUBLIC_KEY_LENGTH]> {
        let now = now_secs();
        if let Some(record) = self.cache.get(destination) {
            if now <= record.received + RATCHET_EXPIRY_SECS {
                return record.ratchet.as_ref().try_into().ok();
            }
            self.cache.remove(destination);
            self.remove_record(destination);
        }

        let record = self.load_record(destination)?;
        if now > record.received + RATCHET_EXPIRY_SECS {
            self.cache.remove(destination);
            self.remove_record(destination);
            return None;
        }
        let ratchet = record.ratchet.as_ref().try_into().ok();
        self.cache.insert(*destination, record);
        ratchet
    }

    pub(crate) fn clean_expired(&mut self, now: f64) {
        self.cache.retain(|_, record| now <= record.received + RATCHET_EXPIRY_SECS);
        if let Ok(entries) = fs::read_dir(&self.ratchet_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(data) = fs::read(&path) {
                    if let Ok(record) = rmp_serde::from_slice::<RatchetRecord>(&data) {
                        if now > record.received + RATCHET_EXPIRY_SECS {
                            let _ = fs::remove_file(path);
                        }
                    }
                }
            }
        }
    }

    fn persist_record(
        &self,
        destination: &AddressHash,
        record: &RatchetRecord,
    ) -> Result<(), RnsError> {
        ensure_dir(&self.ratchet_dir)?;
        let encoded = rmp_serde::to_vec_named(record).map_err(|_| RnsError::PacketError)?;
        let path = self.path_for(destination);
        let tmp_path = path.with_extension("out");
        fs::write(&tmp_path, encoded).map_err(|_| RnsError::PacketError)?;
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        fs::rename(&tmp_path, &path).map_err(|_| RnsError::PacketError)?;
        Ok(())
    }

    fn load_record(&self, destination: &AddressHash) -> Option<RatchetRecord> {
        let path = self.path_for(destination);
        let data = fs::read(path).ok()?;
        rmp_serde::from_slice::<RatchetRecord>(&data).ok()
    }

    fn remove_record(&self, destination: &AddressHash) {
        let path = self.path_for(destination);
        let _ = fs::remove_file(path);
    }

    fn path_for(&self, destination: &AddressHash) -> PathBuf {
        self.ratchet_dir.join(destination.to_hex_string())
    }
}

pub fn encrypt_for_public_key<R: CryptoRngCore + Copy>(
    public_key: &PublicKey,
    salt: &[u8],
    plaintext: &[u8],
    rng: R,
) -> Result<Vec<u8>, RnsError> {
    let secret = EphemeralSecret::random_from_rng(rng);
    let ephemeral_public = PublicKey::from(&secret);
    let shared = secret.diffie_hellman(public_key);
    let derived = DerivedKey::new(&shared, Some(salt));
    let key_bytes = derived.as_bytes();
    let split = key_bytes.len() / 2;

    let fernet = Fernet::new_from_slices(&key_bytes[..split], &key_bytes[split..], rng);
    let mut out =
        vec![
            0u8;
            PUBLIC_KEY_LENGTH + plaintext.len() + FERNET_OVERHEAD_SIZE + FERNET_MAX_PADDING_SIZE
        ];
    out[..PUBLIC_KEY_LENGTH].copy_from_slice(ephemeral_public.as_bytes());
    let token = fernet
        .encrypt(PlainText::from(plaintext), &mut out[PUBLIC_KEY_LENGTH..])
        .map_err(|_| RnsError::CryptoError)?;
    let total = PUBLIC_KEY_LENGTH + token.len();
    out.truncate(total);
    Ok(out)
}

pub fn decrypt_with_private_key(
    private_key: &StaticSecret,
    salt: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RnsError> {
    if ciphertext.len() <= PUBLIC_KEY_LENGTH {
        return Err(RnsError::InvalidArgument);
    }
    let mut pub_bytes = [0u8; PUBLIC_KEY_LENGTH];
    pub_bytes.copy_from_slice(&ciphertext[..PUBLIC_KEY_LENGTH]);
    let ephemeral_public = PublicKey::from(pub_bytes);
    let shared = private_key.diffie_hellman(&ephemeral_public);
    let derived = DerivedKey::new(&shared, Some(salt));
    let key_bytes = derived.as_bytes();
    let split = key_bytes.len() / 2;

    let fernet =
        Fernet::new_from_slices(&key_bytes[..split], &key_bytes[split..], rand_core::OsRng);
    let token = Token::from(&ciphertext[PUBLIC_KEY_LENGTH..]);
    let verified = fernet.verify(token).map_err(|_| RnsError::CryptoError)?;
    let mut out = vec![0u8; ciphertext.len()];
    let plain = fernet.decrypt(verified, &mut out).map_err(|_| RnsError::CryptoError)?;
    Ok(plain.as_bytes().to_vec())
}

pub fn decrypt_with_identity(
    identity: &PrivateIdentity,
    salt: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RnsError> {
    if ciphertext.len() <= PUBLIC_KEY_LENGTH {
        return Err(RnsError::InvalidArgument);
    }
    let mut pub_bytes = [0u8; PUBLIC_KEY_LENGTH];
    pub_bytes.copy_from_slice(&ciphertext[..PUBLIC_KEY_LENGTH]);
    let ephemeral_public = PublicKey::from(pub_bytes);
    let derived = identity.derive_key(&ephemeral_public, Some(salt));
    let key_bytes = derived.as_bytes();
    let split = key_bytes.len() / 2;

    let fernet =
        Fernet::new_from_slices(&key_bytes[..split], &key_bytes[split..], rand_core::OsRng);
    let token = Token::from(&ciphertext[PUBLIC_KEY_LENGTH..]);
    let verified = fernet.verify(token).map_err(|_| RnsError::CryptoError)?;
    let mut out = vec![0u8; ciphertext.len()];
    let plain = fernet.decrypt(verified, &mut out).map_err(|_| RnsError::CryptoError)?;
    Ok(plain.as_bytes().to_vec())
}

pub(crate) fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

fn ensure_dir(path: &Path) -> Result<(), RnsError> {
    fs::create_dir_all(path).map_err(|_| RnsError::PacketError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmpv::Value;
    use tempfile::TempDir;

    #[test]
    fn ratchet_record_encodes_as_bin() {
        let record =
            RatchetRecord { ratchet: ByteBuf::from(vec![1u8; PUBLIC_KEY_LENGTH]), received: 123.0 };
        let encoded = rmp_serde::to_vec_named(&record).expect("encode");
        let mut cursor = std::io::Cursor::new(encoded);
        let value = rmpv::decode::read_value(&mut cursor).expect("decode");
        let map = value.as_map().expect("map");
        let mut ratchet_is_bin = false;
        for (key, val) in map {
            if key.as_str() == Some("ratchet") {
                ratchet_is_bin = matches!(val, Value::Binary(_));
            }
        }
        assert!(ratchet_is_bin, "ratchet should be msgpack binary");
    }

    #[test]
    fn ratchet_store_expiry_removes_entry() {
        let temp = TempDir::new().expect("temp dir");
        let mut store = RatchetStore::new(temp.path().to_path_buf());
        let dest = AddressHash::new_from_rand(rand_core::OsRng);
        let record =
            RatchetRecord { ratchet: ByteBuf::from(vec![2u8; PUBLIC_KEY_LENGTH]), received: 0.0 };
        let encoded = rmp_serde::to_vec_named(&record).expect("encode");
        fs::write(temp.path().join(dest.to_hex_string()), encoded).expect("write");
        let ratchet = store.get(&dest);
        assert!(ratchet.is_none(), "expired ratchet should be ignored");
    }
}
