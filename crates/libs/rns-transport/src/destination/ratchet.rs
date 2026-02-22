use crate::{
    error::RnsError,
    identity::{PrivateIdentity, PUBLIC_KEY_LENGTH},
    ratchets::decrypt_with_private_key,
};
use ed25519_dalek::Signature;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::path::{Path, PathBuf};
use x25519_dalek::{PublicKey, StaticSecret};

pub const RATCHET_LENGTH: usize = PUBLIC_KEY_LENGTH;
const DEFAULT_RATCHET_INTERVAL_SECS: u64 = 30 * 60;
const DEFAULT_RETAINED_RATCHETS: usize = 512;

#[derive(Clone)]
pub(crate) struct RatchetState {
    pub(crate) enabled: bool,
    pub(crate) ratchets: Vec<[u8; RATCHET_LENGTH]>,
    pub(crate) ratchets_path: Option<PathBuf>,
    pub(crate) ratchet_interval_secs: u64,
    pub(crate) retained_ratchets: usize,
    pub(crate) latest_ratchet_time: Option<f64>,
    pub(crate) enforce_ratchets: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedRatchets {
    signature: ByteBuf,
    ratchets: ByteBuf,
}

impl Default for RatchetState {
    fn default() -> Self {
        Self {
            enabled: false,
            ratchets: Vec::new(),
            ratchets_path: None,
            ratchet_interval_secs: DEFAULT_RATCHET_INTERVAL_SECS,
            retained_ratchets: DEFAULT_RETAINED_RATCHETS,
            latest_ratchet_time: None,
            enforce_ratchets: false,
        }
    }
}

impl RatchetState {
    pub(crate) fn enable(
        &mut self,
        identity: &PrivateIdentity,
        path: PathBuf,
    ) -> Result<(), RnsError> {
        self.latest_ratchet_time = Some(0.0);
        self.reload(identity, &path)?;
        self.enabled = true;
        self.ratchets_path = Some(path);
        Ok(())
    }

    pub(crate) fn reload(
        &mut self,
        identity: &PrivateIdentity,
        path: &Path,
    ) -> Result<(), RnsError> {
        if path.exists() {
            let data = std::fs::read(path).map_err(|_| RnsError::PacketError)?;
            let persisted: PersistedRatchets =
                rmp_serde::from_slice(&data).map_err(|_| RnsError::PacketError)?;
            let signature = Signature::from_slice(persisted.signature.as_ref())
                .map_err(|_| RnsError::CryptoError)?;
            identity
                .verify(persisted.ratchets.as_ref(), &signature)
                .map_err(|_| RnsError::IncorrectSignature)?;
            let decoded: Vec<ByteBuf> = rmp_serde::from_slice(persisted.ratchets.as_ref())
                .map_err(|_| RnsError::PacketError)?;
            let mut ratchets = Vec::new();
            for ratchet in decoded {
                if ratchet.len() == RATCHET_LENGTH {
                    let mut bytes = [0u8; RATCHET_LENGTH];
                    bytes.copy_from_slice(ratchet.as_ref());
                    ratchets.push(bytes);
                }
            }
            self.ratchets = ratchets;
            return Ok(());
        }

        self.ratchets = Vec::new();
        self.persist(identity, path)?;
        Ok(())
    }

    fn persist(&self, identity: &PrivateIdentity, path: &Path) -> Result<(), RnsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| RnsError::PacketError)?;
        }
        let packed = pack_ratchets(&self.ratchets)?;
        let signature = identity.sign(&packed).to_bytes();
        let persisted = PersistedRatchets {
            signature: ByteBuf::from(signature.to_vec()),
            ratchets: ByteBuf::from(packed),
        };
        let encoded = rmp_serde::to_vec(&persisted).map_err(|_| RnsError::PacketError)?;
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, encoded).map_err(|_| RnsError::PacketError)?;
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        std::fs::rename(&tmp_path, path).map_err(|_| RnsError::PacketError)?;
        Ok(())
    }

    pub(crate) fn rotate_if_needed(
        &mut self,
        identity: &PrivateIdentity,
        now: f64,
    ) -> Result<(), RnsError> {
        if !self.enabled {
            return Ok(());
        }
        let last = self.latest_ratchet_time.unwrap_or(0.0);
        if self.ratchets.is_empty() || now > last + self.ratchet_interval_secs as f64 {
            let secret = StaticSecret::random_from_rng(OsRng);
            self.ratchets.insert(0, secret.to_bytes());
            self.latest_ratchet_time = Some(now);
            if self.ratchets.len() > self.retained_ratchets {
                self.ratchets.truncate(self.retained_ratchets);
            }
            if let Some(path) = self.ratchets_path.clone() {
                self.persist(identity, &path)?;
            }
        }
        Ok(())
    }

    pub(crate) fn current_ratchet_public(&self) -> Option<[u8; RATCHET_LENGTH]> {
        let ratchet = self.ratchets.first()?;
        let secret = StaticSecret::from(*ratchet);
        let public = PublicKey::from(&secret);
        let mut bytes = [0u8; RATCHET_LENGTH];
        bytes.copy_from_slice(public.as_bytes());
        Some(bytes)
    }
}

fn pack_ratchets(ratchets: &[[u8; RATCHET_LENGTH]]) -> Result<Vec<u8>, RnsError> {
    let list: Vec<ByteBuf> = ratchets.iter().map(|bytes| ByteBuf::from(bytes.to_vec())).collect();
    rmp_serde::to_vec(&list).map_err(|_| RnsError::PacketError)
}

pub(crate) fn try_decrypt_with_ratchets(
    state: &RatchetState,
    salt: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    for ratchet in &state.ratchets {
        let secret = StaticSecret::from(*ratchet);
        if let Ok(plaintext) = decrypt_with_private_key(&secret, salt, ciphertext) {
            return Some(plaintext);
        }
    }
    None
}
