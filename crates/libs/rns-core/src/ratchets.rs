use alloc::vec;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use rand_core::CryptoRngCore;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

use crate::crypt::fernet::{
    Fernet, PlainText, Token, FERNET_MAX_PADDING_SIZE, FERNET_OVERHEAD_SIZE,
};
use crate::error::RnsError;
use crate::identity::{DerivedKey, PrivateIdentity, PUBLIC_KEY_LENGTH};

pub fn encrypt_for_public_key<R: CryptoRngCore + Copy>(
    public_key: &PublicKey,
    salt: &[u8],
    plaintext: &[u8],
    rng: R,
) -> Result<Vec<u8>, RnsError> {
    let mut out =
        vec![
            0u8;
            PUBLIC_KEY_LENGTH + plaintext.len() + FERNET_OVERHEAD_SIZE + FERNET_MAX_PADDING_SIZE
        ];
    let total = encrypt_for_public_key_into(public_key, salt, plaintext, &mut out, rng)?.len();
    out.truncate(total);
    Ok(out)
}

pub fn encrypt_for_public_key_into<'a, R: CryptoRngCore + Copy>(
    public_key: &PublicKey,
    salt: &[u8],
    plaintext: &[u8],
    out: &'a mut [u8],
    rng: R,
) -> Result<&'a [u8], RnsError> {
    let secret = EphemeralSecret::random_from_rng(rng);
    let ephemeral_public = PublicKey::from(&secret);
    let shared = secret.diffie_hellman(public_key);
    let derived = DerivedKey::new(&shared, Some(salt));
    let key_bytes = derived.as_bytes();
    let split = key_bytes.len() / 2;

    let fernet = Fernet::new_from_slices(&key_bytes[..split], &key_bytes[split..], rng);
    if out.len() < PUBLIC_KEY_LENGTH {
        return Err(RnsError::InvalidArgument);
    }
    out[..PUBLIC_KEY_LENGTH].copy_from_slice(ephemeral_public.as_bytes());
    let token = fernet
        .encrypt(PlainText::from(plaintext), &mut out[PUBLIC_KEY_LENGTH..])
        .map_err(|_| RnsError::CryptoError)?;
    let total = PUBLIC_KEY_LENGTH + token.len();
    Ok(&out[..total])
}

pub fn decrypt_with_private_key(
    private_key: &StaticSecret,
    salt: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RnsError> {
    let mut out = vec![0u8; ciphertext.len()];
    let plain_len = decrypt_with_private_key_into(private_key, salt, ciphertext, &mut out)?.len();
    out.truncate(plain_len);
    Ok(out)
}

pub fn decrypt_with_private_key_into<'a>(
    private_key: &StaticSecret,
    salt: &[u8],
    ciphertext: &[u8],
    out: &'a mut [u8],
) -> Result<&'a [u8], RnsError> {
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
    let plain = fernet.decrypt(verified, out).map_err(|_| RnsError::CryptoError)?;
    Ok(plain.as_bytes())
}

pub fn decrypt_with_identity(
    identity: &PrivateIdentity,
    salt: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RnsError> {
    let mut out = vec![0u8; ciphertext.len()];
    let plain_len = decrypt_with_identity_into(identity, salt, ciphertext, &mut out)?.len();
    out.truncate(plain_len);
    Ok(out)
}

pub fn decrypt_with_identity_into<'a>(
    identity: &PrivateIdentity,
    salt: &[u8],
    ciphertext: &[u8],
    out: &'a mut [u8],
) -> Result<&'a [u8], RnsError> {
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
    let plain = fernet.decrypt(verified, out).map_err(|_| RnsError::CryptoError)?;
    Ok(plain.as_bytes())
}

pub(crate) fn now_secs() -> Option<f64> {
    if TIME_OVERRIDE_SET.load(Ordering::Acquire) {
        return Some(f64::from(TIME_OVERRIDE_SECS.load(Ordering::Acquire)));
    }
    #[cfg(feature = "std")]
    {
        Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        )
    }
    #[cfg(not(feature = "std"))]
    {
        // Embedded time contract (issue #518): there is no system clock
        // in no_std builds. Rather than silently returning zero — which
        // would pin announce timestamps to the epoch and freeze ratchet
        // rotation after the first key, silently defeating expiry and
        // replay-window checks — report the absence of a time source so
        // callers fail explicitly until `set_time_override` is called.
        None
    }
}

// Embedded time contract state. Whole-second resolution is sufficient:
// announce random blobs encode whole seconds and ratchet intervals are
// seconds-granular. `AtomicU32` keeps the contract usable on embedded
// targets without 64-bit atomics (e.g. thumbv6m).
static TIME_OVERRIDE_SECS: AtomicU32 = AtomicU32::new(0);
static TIME_OVERRIDE_SET: AtomicBool = AtomicBool::new(false);

/// Installs (or replaces) the wall-clock time used by announce
/// timestamps and ratchet rotation, at whole-second resolution.
///
/// This is the embedded time contract for `no_std` builds (issue #518):
/// the embedding application must call this with the current unix time
/// (e.g. from an RTC) before using announce/ratchet flows, and call it
/// again as time advances. Without it, timestamp-dependent operations
/// fail with [`RnsError::TimeSourceUnavailable`] instead of silently
/// using a zero timestamp.
///
/// In `std` builds the system clock is used by default and this serves
/// as a deterministic override, primarily for tests.
///
/// Non-finite or pre-epoch values are clamped to `0`; values beyond
/// `u32::MAX` seconds are clamped to `u32::MAX`.
pub fn set_time_override(now_secs: f64) {
    let secs = if now_secs.is_finite() && now_secs > 0.0 {
        (now_secs as u64).min(u64::from(u32::MAX)) as u32
    } else {
        0
    };
    TIME_OVERRIDE_SECS.store(secs, Ordering::Release);
    TIME_OVERRIDE_SET.store(true, Ordering::Release);
}

/// Removes a previously installed time override, restoring the default
/// clock behavior (system time in `std` builds, "no time source" in
/// `no_std` builds).
pub fn clear_time_override() {
    TIME_OVERRIDE_SET.store(false, Ordering::Release);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{clear_time_override, now_secs, set_time_override};

    /// Serializes tests that install the global time override against
    /// each other and against tests that rely on the real clock (e.g.
    /// `destination::tests::announce_random_blob_matches_python_layout`).
    pub(crate) fn time_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        &LOCK
    }

    #[test]
    fn now_secs_uses_override_once_embedded_time_source_is_installed() {
        let _guard = time_test_lock().lock().expect("time test lock");
        set_time_override(1_700_000_000.7);
        assert_eq!(
            now_secs(),
            Some(1_700_000_000.0),
            "override applies at whole-second resolution"
        );
        set_time_override(1_700_000_123.0);
        assert_eq!(
            now_secs(),
            Some(1_700_000_123.0),
            "the source can be refreshed as time advances"
        );
        clear_time_override();
        assert_ne!(now_secs(), Some(1_700_000_123.0), "std builds fall back to the system clock");
    }

    #[test]
    fn time_override_clamps_out_of_range_values() {
        let _guard = time_test_lock().lock().expect("time test lock");
        set_time_override(f64::NAN);
        assert_eq!(now_secs(), Some(0.0));
        set_time_override(-42.0);
        assert_eq!(now_secs(), Some(0.0));
        set_time_override(f64::from(u32::MAX) + 1000.0);
        assert_eq!(now_secs(), Some(f64::from(u32::MAX)));
        clear_time_override();
    }
}
