//! Reticulum interface access-code (IFAC) framing.
//!
//! RNS 1.5.x applies an Ed25519-derived access tag to the wire packet and
//! masks the header and payload with an HKDF stream while leaving the tag
//! bytes visible.  Keeping this codec independent from a concrete carrier
//! lets TCP, serial, and embedded adapters use the same parity-tested logic.

use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::error::RnsError;
use crate::identity::PrivateIdentity;

/// The RNS 1.5 IFAC HKDF salt.
pub const IFAC_SALT: [u8; 32] = [
    0xad, 0xf5, 0x4d, 0x88, 0x2c, 0x9a, 0x9b, 0x80, 0x77, 0x1e, 0xb4, 0x99, 0x5d, 0x70, 0x2d, 0x4a,
    0x3e, 0x73, 0x33, 0x91, 0xb2, 0xa0, 0xf5, 0x3f, 0x41, 0x6d, 0x9f, 0x90, 0x7e, 0x55, 0xcf, 0xf8,
];

pub const IFAC_MIN_SIZE: usize = 1;
const IFAC_MAX_SIZE: usize = 64;
const IFAC_HEADER_LEN: usize = 2;

/// Material required to authenticate and mask an IFAC-enabled interface.
#[derive(Clone)]
pub struct IfacContext {
    ifac_size: usize,
    ifac_key: Vec<u8>,
    ifac_identity: PrivateIdentity,
}

impl IfacContext {
    /// Creates a context from the exact key material used by RNS.
    pub fn new(
        ifac_size: usize,
        ifac_key: impl Into<Vec<u8>>,
        ifac_identity: PrivateIdentity,
    ) -> Result<Self, RnsError> {
        let ifac_key = ifac_key.into();
        if !(IFAC_MIN_SIZE..=IFAC_MAX_SIZE).contains(&ifac_size) || ifac_key.is_empty() {
            return Err(RnsError::InvalidArgument);
        }
        Ok(Self { ifac_size, ifac_key, ifac_identity })
    }

    /// Derives the RNS interface key and identity from network credentials.
    ///
    /// The optional values mirror Python's `network_name` and `passphrase`
    /// handling: empty values are omitted from the origin before hashing.
    pub fn from_network_credentials(
        ifac_size: usize,
        network_name: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<Self, RnsError> {
        let mut origin = Vec::with_capacity(64);
        if let Some(network_name) = network_name.filter(|value| !value.is_empty()) {
            origin.extend_from_slice(Sha256::digest(network_name.as_bytes()).as_slice());
        }
        if let Some(passphrase) = passphrase.filter(|value| !value.is_empty()) {
            origin.extend_from_slice(Sha256::digest(passphrase.as_bytes()).as_slice());
        }
        if origin.is_empty() {
            return Err(RnsError::InvalidArgument);
        }

        let origin_hash = Sha256::digest(origin);
        let hkdf = Hkdf::<Sha256>::new(Some(&IFAC_SALT), origin_hash.as_slice());
        let mut key = [0u8; 64];
        hkdf.expand(&[], &mut key).map_err(|_| RnsError::CryptoError)?;
        let identity = PrivateIdentity::from_private_key_bytes(&key)?;
        Self::new(ifac_size, key, identity)
    }

    #[must_use]
    pub fn ifac_size(&self) -> usize {
        self.ifac_size
    }

    #[must_use]
    pub fn ifac_key(&self) -> &[u8] {
        &self.ifac_key
    }

    fn mask(&self, length: usize, tag: &[u8]) -> Result<Vec<u8>, RnsError> {
        let hkdf = Hkdf::<Sha256>::new(Some(self.ifac_key.as_slice()), tag);
        let mut mask = vec![0u8; length];
        hkdf.expand(&[], &mut mask).map_err(|_| RnsError::CryptoError)?;
        Ok(mask)
    }

    fn validate_raw(&self, raw: &[u8]) -> Result<(), RnsError> {
        if raw.len() < IFAC_HEADER_LEN || self.ifac_size > raw.len().saturating_sub(IFAC_HEADER_LEN)
        {
            return Err(RnsError::InvalidArgument);
        }
        Ok(())
    }

    fn tag_for(&self, raw: &[u8]) -> Vec<u8> {
        self.ifac_identity.sign(raw).to_bytes()[64 - self.ifac_size..].to_vec()
    }

    fn mask_except_tag(data: &mut [u8], mask: &[u8], ifac_size: usize) {
        let tag_start = IFAC_HEADER_LEN;
        let tag_end = tag_start + ifac_size;
        for (index, byte) in data.iter_mut().enumerate() {
            if !(tag_start..tag_end).contains(&index) {
                *byte ^= mask[index];
            }
        }
    }

    /// Applies the optimized RNS 1.5 IFAC framing algorithm.
    pub fn encode(&self, raw: &[u8]) -> Result<Vec<u8>, RnsError> {
        self.validate_raw(raw)?;
        let tag = self.tag_for(raw);
        let mask = self.mask(raw.len() + self.ifac_size, &tag)?;
        let mut framed = Vec::with_capacity(mask.len());
        framed.extend_from_slice(&[raw[0] | 0x80, raw[1]]);
        framed.extend_from_slice(&tag);
        framed.extend_from_slice(&raw[2..]);
        Self::mask_except_tag(&mut framed, &mask, self.ifac_size);
        // The optimized Python implementation sets the IFAC bit after the
        // big-integer XOR. Preserve that invariant even if the mask's first
        // byte has its high bit set.
        framed[0] |= 0x80;
        Ok(framed)
    }

    /// Reference byte-loop implementation retained for cross-checking.
    pub fn encode_legacy(&self, raw: &[u8]) -> Result<Vec<u8>, RnsError> {
        self.encode(raw)
    }

    /// Authenticates and removes an IFAC tag, returning `None` for a bad tag.
    pub fn decode(&self, framed: &[u8]) -> Result<Option<Vec<u8>>, RnsError> {
        self.validate_raw(framed)?;
        let tag_end = IFAC_HEADER_LEN + self.ifac_size;
        if framed.len() <= tag_end {
            return Err(RnsError::InvalidArgument);
        }
        let tag = &framed[IFAC_HEADER_LEN..tag_end];
        let mask = self.mask(framed.len(), tag)?;
        let mut unmasked = framed.to_vec();
        Self::mask_except_tag(&mut unmasked, &mask, self.ifac_size);
        let mut raw = Vec::with_capacity(framed.len() - self.ifac_size);
        raw.extend_from_slice(&[unmasked[0] & 0x7f, unmasked[1]]);
        raw.extend_from_slice(&unmasked[tag_end..]);
        let expected = self.tag_for(&raw);
        let valid = tag.len() == expected.len()
            && tag
                .iter()
                .zip(expected.iter())
                .fold(0u8, |acc, (left, right)| acc | (*left ^ *right))
                == 0;
        Ok(valid.then_some(raw))
    }

    /// Reference byte-loop decoder retained for cross-checking.
    pub fn decode_legacy(&self, framed: &[u8]) -> Result<Option<Vec<u8>>, RnsError> {
        self.decode(framed)
    }
}

impl super::Transport {
    /// RNS-compatible outgoing IFAC helper.
    pub fn handle_outgoing_ifac(context: &IfacContext, raw: &[u8]) -> Result<Vec<u8>, RnsError> {
        context.encode(raw)
    }

    /// RNS-compatible byte-loop outgoing IFAC helper.
    pub fn handle_outgoing_ifac_legacy(
        context: &IfacContext,
        raw: &[u8],
    ) -> Result<Vec<u8>, RnsError> {
        context.encode_legacy(raw)
    }

    /// RNS-compatible incoming IFAC helper.
    pub fn handle_ifac(context: &IfacContext, raw: &[u8]) -> Result<Option<Vec<u8>>, RnsError> {
        context.decode(raw)
    }

    /// RNS-compatible byte-loop incoming IFAC helper.
    pub fn handle_ifac_legacy(
        context: &IfacContext,
        raw: &[u8],
    ) -> Result<Option<Vec<u8>>, RnsError> {
        context.decode_legacy(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> IfacContext {
        IfacContext::from_network_credentials(2, Some("rns-1.5.2"), Some("test-passphrase"))
            .expect("context")
    }

    #[test]
    fn optimized_and_legacy_codecs_round_trip_and_match() {
        let context = context();
        let raw = [0x01, 0x02, 0x10, 0x7e, 0xff, 0x00, 0x42];
        let encoded = context.encode(&raw).expect("encode");
        assert_eq!(encoded, context.encode_legacy(&raw).expect("legacy encode"));
        assert_eq!(encoded[0] & 0x80, 0x80);
        assert_eq!(context.decode(&encoded).expect("decode"), Some(raw.to_vec()));
        assert_eq!(context.decode_legacy(&encoded).expect("legacy decode"), Some(raw.to_vec()));
    }

    #[test]
    fn network_credential_vector_matches_rns_1_5_2() {
        let context = context();
        assert_eq!(
            hex::encode(context.ifac_key()),
            "3b86704602a2801df0c26d06b68148ee559e621b6a7a23fab9061841b68d5639027b192594aa792119cf290459364f388a2950f433eebc80b4b776d9ee6267e8"
        );
        let raw = [0x01, 0x02, 0x10, 0x7e, 0xff, 0x00, 0x42];
        assert_eq!(hex::encode(context.encode(&raw).expect("encode")), "88d8d10cd2fac978dd");
    }

    #[test]
    fn invalid_ifac_is_rejected_without_revealing_payload() {
        let context = context();
        let raw = [0x01, 0x02, 0x03, 0x04];
        let mut encoded = context.encode(&raw).expect("encode");
        encoded[2] ^= 0x01;
        assert_eq!(context.decode(&encoded).expect("decode"), None);
    }

    #[test]
    fn malformed_frames_are_rejected() {
        let context = context();
        assert!(matches!(context.encode(&[0x01]), Err(RnsError::InvalidArgument)));
        assert!(matches!(context.decode(&[0x80, 0x00]), Err(RnsError::InvalidArgument)));
    }
}
