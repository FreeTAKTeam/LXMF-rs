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
        let packed = pack_ratchets(&self.ratchets)?;
        let signature = identity.sign(&packed).to_bytes();
        let persisted = PersistedRatchets {
            signature: ByteBuf::from(signature.to_vec()),
            ratchets: ByteBuf::from(packed),
        };
        let encoded = rmp_serde::to_vec(&persisted).map_err(|_| RnsError::PacketError)?;
        rns_core::secure_storage::atomic_write_private(path, &encoded)
            .map_err(|_| RnsError::PacketError)?;
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

#[cfg(test)]
mod tests {
    use super::RatchetState;
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    #[test]
    fn persisted_ratchets_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ratchets").join("destination.ratchets");
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let expected = vec![[0x42; super::RATCHET_LENGTH]];
        let state = RatchetState { ratchets: expected.clone(), ..RatchetState::default() };

        state.persist(&identity, &path).expect("persist ratchets");
        let mut loaded = RatchetState::default();
        loaded.reload(&identity, &path).expect("reload ratchets");

        assert_eq!(loaded.ratchets, expected);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_ratchets_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let directory = temp.path().join("ratchets");
        std::fs::create_dir(&directory).expect("create permissive ratchet directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .expect("set permissive directory mode");
        let path = directory.join("destination.ratchets");
        let identity = PrivateIdentity::new_from_rand(OsRng);

        RatchetState::default().persist(&identity, &path).expect("persist ratchets");

        let directory_mode =
            std::fs::metadata(&directory).expect("ratchet directory metadata").permissions().mode()
                & 0o777;
        let file_mode =
            std::fs::metadata(&path).expect("ratchet file metadata").permissions().mode() & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_ratchets_do_not_follow_legacy_temp_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let directory = temp.path().join("ratchets");
        std::fs::create_dir(&directory).expect("create ratchet directory");
        let path = directory.join("destination.ratchets");
        let victim = temp.path().join("victim");
        std::fs::write(&victim, b"unchanged").expect("write victim");
        let legacy_tmp = path.with_extension("tmp");
        symlink(&victim, &legacy_tmp).expect("create legacy temp symlink");
        let identity = PrivateIdentity::new_from_rand(OsRng);

        RatchetState::default().persist(&identity, &path).expect("persist ratchets");

        assert_eq!(std::fs::read(&victim).expect("read victim"), b"unchanged");
        assert!(legacy_tmp.is_symlink());
        assert!(path.is_file());
    }
}
