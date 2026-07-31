use crate::{
    error::RnsError,
    identity::{PrivateIdentity, PUBLIC_KEY_LENGTH},
    ratchets::decrypt_with_private_key,
};
use alloc::vec::Vec;
#[cfg(feature = "std")]
use ed25519_dalek::Signature;
use rand_core::OsRng;
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use serde_bytes::ByteBuf;
#[cfg(feature = "std")]
use std::path::{Path, PathBuf};
use x25519_dalek::{PublicKey, StaticSecret};

pub const RATCHET_LENGTH: usize = PUBLIC_KEY_LENGTH;
const DEFAULT_RATCHET_INTERVAL_SECS: u64 = 30 * 60;
const DEFAULT_RETAINED_RATCHETS: usize = 512;

#[derive(Clone)]
pub(crate) struct RatchetState {
    pub(crate) enabled: bool,
    pub(crate) ratchets: Vec<[u8; RATCHET_LENGTH]>,
    #[cfg(feature = "std")]
    pub(crate) ratchets_path: Option<PathBuf>,
    pub(crate) ratchet_interval_secs: u64,
    pub(crate) retained_ratchets: usize,
    pub(crate) latest_ratchet_time: Option<f64>,
    pub(crate) enforce_ratchets: bool,
}

#[cfg(feature = "std")]
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
            #[cfg(feature = "std")]
            ratchets_path: None,
            ratchet_interval_secs: DEFAULT_RATCHET_INTERVAL_SECS,
            retained_ratchets: DEFAULT_RETAINED_RATCHETS,
            latest_ratchet_time: None,
            enforce_ratchets: false,
        }
    }
}

impl RatchetState {
    #[cfg(feature = "std")]
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

    #[cfg(feature = "std")]
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

    #[cfg(feature = "std")]
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
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(path).map_err(|_| RnsError::PacketError)?;
        }
        std::fs::rename(&tmp_path, path).map_err(|_| RnsError::PacketError)?;
        Ok(())
    }

    pub(crate) fn rotate_if_needed(
        &mut self,
        identity: &PrivateIdentity,
        now: f64,
    ) -> Result<(), RnsError> {
        #[cfg(not(feature = "std"))]
        let _ = identity;
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
            #[cfg(feature = "std")]
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

#[cfg(feature = "std")]
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
    use super::{RatchetState, DEFAULT_RATCHET_INTERVAL_SECS};
    use crate::identity::PrivateIdentity;
    use rand_core::OsRng;

    // no_std-focused tests for issue #518: ratchet rotation is driven
    // entirely by the injected `now`, so the embedded time contract can
    // be validated deterministically. A frozen or zero clock must never
    // silently rotate (or refuse to rotate) — rotation happens exactly
    // when the injected time crosses the interval boundary.
    #[test]
    fn rotate_if_needed_rotates_exactly_on_injected_interval_boundary() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let mut state = RatchetState { enabled: true, ..RatchetState::default() };

        state.rotate_if_needed(&identity, 1_000.0).expect("first rotation");
        assert_eq!(state.ratchets.len(), 1, "empty state rotates regardless of clock");
        let first = state.ratchets.clone();

        state
            .rotate_if_needed(&identity, 1_000.0 + DEFAULT_RATCHET_INTERVAL_SECS as f64 - 1.0)
            .expect("within interval");
        assert_eq!(state.ratchets, first, "no rotation before the interval elapses");

        state
            .rotate_if_needed(&identity, 1_000.0 + DEFAULT_RATCHET_INTERVAL_SECS as f64 + 1.0)
            .expect("past interval");
        assert_eq!(state.ratchets.len(), 2);
        assert_ne!(
            state.ratchets[0], first[0],
            "rotation past the interval installs a new ratchet"
        );
    }

    #[test]
    fn rotate_if_needed_with_zero_clock_does_not_falsely_rotate_or_expire() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let mut state = RatchetState { enabled: true, ..RatchetState::default() };

        // The issue-518 no_std failure mode: a silent 0.0 clock rotates
        // once and then never again (0 > 0 + interval is false), freezing
        // the replay window. With the injected-time contract the caller
        // controls `now`, and a zero value must behave as "no time
        // elapsed": rotate the empty state once, then hold steady.
        state.rotate_if_needed(&identity, 0.0).expect("zero clock");
        assert_eq!(state.ratchets.len(), 1);
        let only = state.ratchets.clone();
        for _ in 0..3 {
            state.rotate_if_needed(&identity, 0.0).expect("zero clock again");
        }
        assert_eq!(state.ratchets, only, "a frozen clock must not silently rotate or expire");
    }

    #[test]
    fn rotate_if_needed_noops_when_ratchets_disabled() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let mut state = RatchetState::default();

        state.rotate_if_needed(&identity, 1_000.0).expect("disabled state");
        assert!(state.ratchets.is_empty());
    }
}
