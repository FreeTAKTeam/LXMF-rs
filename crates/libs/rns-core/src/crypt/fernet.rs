use core::cmp;
use core::convert::From;

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::BlockDecryptMut;
use aes::cipher::BlockSizeUser;
use aes::cipher::Key;
use aes::cipher::Unsigned;
use cbc::cipher::BlockEncryptMut;
use cbc::cipher::KeyIvInit;
use crypto_common::{IvSizeUser, KeySizeUser, OutputSizeUser};
use hmac::{Hmac, Mac};
use rand_core::CryptoRngCore;
use sha2::Sha256;

use crate::error::RnsError;

#[cfg(feature = "fernet-aes128")]
type AesAlgo = aes::Aes128;
#[cfg(not(feature = "fernet-aes128"))]
type AesAlgo = aes::Aes256;

type AesCbcEnc = cbc::Encryptor<AesAlgo>;
type AesCbcDec = cbc::Decryptor<AesAlgo>;
type AesKey = Key<AesAlgo>;

type HmacSha256 = Hmac<Sha256>;

const HMAC_OUT_SIZE: usize = <<HmacSha256 as OutputSizeUser>::OutputSize as Unsigned>::USIZE;
const AES_KEY_SIZE: usize = <<AesAlgo as KeySizeUser>::KeySize as Unsigned>::USIZE;
const IV_KEY_SIZE: usize = <<AesCbcEnc as IvSizeUser>::IvSize as Unsigned>::USIZE;
const AES_BLOCK_SIZE: usize = <<AesAlgo as BlockSizeUser>::BlockSize as Unsigned>::USIZE;
pub const FERNET_OVERHEAD_SIZE: usize = IV_KEY_SIZE + HMAC_OUT_SIZE;
pub const FERNET_MAX_PADDING_SIZE: usize = AES_BLOCK_SIZE;

pub struct PlainText<'a>(&'a [u8]);
pub struct VerifiedToken<'a>(&'a [u8]);
pub struct Token<'a>(&'a [u8]);

// This class provides a slightly modified implementation of the Fernet spec
// found at: https://github.com/fernet/spec/blob/master/Spec.md
//
// According to the spec, a Fernet token includes a one byte VERSION and
// eight byte TIMESTAMP field at the start of each token. These fields are
// not relevant to Reticulum. They are therefore stripped from this
// implementation, since they incur overhead and leak initiator metadata.
pub struct Fernet<R: CryptoRngCore> {
    rng: R,
    sign_key: [u8; AES_KEY_SIZE],
    enc_key: AesKey,
}

impl<'a> PlainText<'a> {
    pub fn as_slice(&self) -> &'a [u8] {
        self.0
    }
}

impl<'a> From<&'a str> for PlainText<'a> {
    fn from(item: &'a str) -> Self {
        Self(item.as_bytes())
    }
}

impl<'a> From<&'a [u8]> for PlainText<'a> {
    fn from(item: &'a [u8]) -> Self {
        Self(item)
    }
}

impl<'a> PlainText<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.0
    }
}

impl<'a> Token<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> From<&'a [u8]> for Token<'a> {
    fn from(item: &'a [u8]) -> Self {
        Self(item)
    }
}

impl<R: CryptoRngCore + Copy> Fernet<R> {
    pub fn new(sign_key: [u8; AES_KEY_SIZE], enc_key: AesKey, rng: R) -> Self {
        Self { rng, sign_key, enc_key }
    }

    pub fn new_from_slices(sign_key: &[u8], enc_key: &[u8], rng: R) -> Self {
        let mut sign_key_bytes = [0u8; AES_KEY_SIZE];
        let sign_len = cmp::min(AES_KEY_SIZE, sign_key.len());
        sign_key_bytes[..sign_len].copy_from_slice(&sign_key[..sign_len]);

        let mut enc_key_bytes = [0u8; AES_KEY_SIZE];
        let enc_len = cmp::min(AES_KEY_SIZE, enc_key.len());
        enc_key_bytes[..enc_len].copy_from_slice(&enc_key[..enc_len]);

        Self { rng, sign_key: sign_key_bytes, enc_key: enc_key_bytes.into() }
    }

    pub fn new_rand(mut rng: R) -> Self {
        let mut sign_key = [0u8; AES_KEY_SIZE];
        rng.fill_bytes(&mut sign_key);
        let enc_key = AesCbcEnc::generate_key(&mut rng);

        Self { rng, sign_key, enc_key }
    }

    pub fn encrypt<'a>(
        &self,
        text: PlainText,
        out_buf: &'a mut [u8],
    ) -> Result<Token<'a>, RnsError> {
        let block_count = text
            .0
            .len()
            .checked_div(AES_BLOCK_SIZE)
            .and_then(|blocks| blocks.checked_add(1))
            .ok_or(RnsError::InvalidArgument)?;
        let padded_cipher_len =
            block_count.checked_mul(AES_BLOCK_SIZE).ok_or(RnsError::InvalidArgument)?;
        let required_len =
            FERNET_OVERHEAD_SIZE.checked_add(padded_cipher_len).ok_or(RnsError::InvalidArgument)?;

        if out_buf.len() < required_len {
            return Err(RnsError::InvalidArgument);
        }

        let mut out_len = 0;

        // Generate random IV
        let iv = AesCbcEnc::generate_iv(self.rng);
        out_buf[..iv.len()].copy_from_slice(iv.as_slice());

        out_len += iv.len();

        let chiper_len = AesCbcEnc::new(&self.enc_key, &iv)
            .encrypt_padded_b2b_mut::<Pkcs7>(text.0, &mut out_buf[out_len..])
            .map_err(|_| RnsError::InvalidArgument)?
            .len();

        out_len += chiper_len;

        let mut hmac = <HmacSha256 as Mac>::new_from_slice(&self.sign_key)
            .map_err(|_| RnsError::InvalidArgument)?;

        hmac.update(&out_buf[..out_len]);

        let tag = hmac.finalize().into_bytes();

        out_buf[out_len..out_len + tag.len()].copy_from_slice(tag.as_slice());
        out_len += tag.len();

        Ok(Token(&out_buf[..out_len]))
    }

    pub fn verify<'a>(&self, token: Token<'a>) -> Result<VerifiedToken<'a>, RnsError> {
        let token_data = token.0;

        if token_data.len() <= FERNET_OVERHEAD_SIZE {
            return Err(RnsError::InvalidArgument);
        }

        let expected_tag = &token_data[token_data.len() - HMAC_OUT_SIZE..];

        let mut hmac = <HmacSha256 as Mac>::new_from_slice(&self.sign_key)
            .map_err(|_| RnsError::InvalidArgument)?;

        hmac.update(&token_data[..token_data.len() - HMAC_OUT_SIZE]);

        let actual_tag = hmac.finalize().into_bytes();

        let valid = expected_tag
            .iter()
            .zip(actual_tag.as_slice())
            .map(|(x, y)| x.cmp(y))
            .find(|&ord| ord != cmp::Ordering::Equal)
            .unwrap_or(actual_tag.len().cmp(&expected_tag.len()))
            == cmp::Ordering::Equal;

        if valid {
            Ok(VerifiedToken(token_data))
        } else {
            Err(RnsError::IncorrectSignature)
        }
    }

    pub fn decrypt<'a, 'b>(
        &self,
        token: VerifiedToken<'a>,
        out_buf: &'b mut [u8],
    ) -> Result<PlainText<'b>, RnsError> {
        let token_data = token.0;

        if token_data.len() <= FERNET_OVERHEAD_SIZE {
            return Err(RnsError::InvalidArgument);
        }

        let tag_start_index = token_data.len() - HMAC_OUT_SIZE;

        let iv: [u8; IV_KEY_SIZE] =
            token_data[..IV_KEY_SIZE].try_into().map_err(|_| RnsError::InvalidArgument)?;

        let ciphertext = &token_data[IV_KEY_SIZE..tag_start_index];

        let msg = AesCbcDec::new(&self.enc_key, &iv.into())
            .decrypt_padded_b2b_mut::<Pkcs7>(ciphertext, out_buf)
            .map_err(|_| RnsError::CryptoError)?;

        Ok(PlainText(msg))
    }
}

#[cfg(test)]
mod tests {
    use crate::crypt::fernet::{Fernet, AES_BLOCK_SIZE, FERNET_OVERHEAD_SIZE};
    use core::str;
    use rand_core::OsRng;

    #[test]
    fn encrypt_then_decrypt() {
        const BUF_SIZE: usize = 4096;

        let fernet = Fernet::new_rand(OsRng);

        let out_msg: &str = "#FERNET_TEST_MESSAGE#";

        let mut out_buf = [0u8; BUF_SIZE];

        let token = fernet.encrypt(out_msg.into(), &mut out_buf[..]).expect("cipher token");

        let token = fernet.verify(token).expect("verified token");

        let mut in_buf = [0u8; BUF_SIZE];
        let in_msg = str::from_utf8(fernet.decrypt(token, &mut in_buf).expect("decoded token").0)
            .expect("valid string");

        assert_eq!(in_msg, out_msg);
    }

    #[test]
    fn small_buffer() {
        let fernet = Fernet::new_rand(OsRng);

        let test_msg: &str = "#FERNET_TEST_MESSAGE#";

        let mut out_buf = [0u8; 12];
        assert!(fernet.encrypt(test_msg.into(), &mut out_buf[..]).is_err());
    }

    #[test]
    fn rejects_buffer_too_small_for_padding_without_panicking() {
        let fernet = Fernet::new_rand(OsRng);
        let test_msg: &str = "hello";

        // More than overhead but less than required encrypted token size.
        let mut out_buf = [0u8; FERNET_OVERHEAD_SIZE + AES_BLOCK_SIZE - 1];
        assert!(fernet.encrypt(test_msg.into(), &mut out_buf[..]).is_err());
    }
}
