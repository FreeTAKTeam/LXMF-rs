#[cfg(not(feature = "fernet-aes128"))]
use rand_core::{CryptoRng, RngCore};

#[cfg(not(feature = "fernet-aes128"))]
#[derive(Clone, Copy)]
struct FixedRng(u8);

#[cfg(not(feature = "fernet-aes128"))]
impl RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes([self.0; 4])
    }

    fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes([self.0; 8])
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for byte in dest.iter_mut() {
            *byte = self.0;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

#[cfg(not(feature = "fernet-aes128"))]
impl CryptoRng for FixedRng {}

#[test]
#[cfg(not(feature = "fernet-aes128"))]
fn encrypted_payload_matches_fixture() {
    let key = std::fs::read("tests/fixtures/python/reticulum/crypto_key.bin").unwrap();
    let plaintext = std::fs::read("tests/fixtures/python/reticulum/plaintext.bin").unwrap();
    let expected = std::fs::read("tests/fixtures/python/reticulum/encrypted_payload.bin").unwrap();

    let sign_key = &key[..32];
    let enc_key = &key[32..];
    let fernet =
        reticulum::crypt::fernet::Fernet::new_from_slices(sign_key, enc_key, FixedRng(0x42));

    let out_len = plaintext.len()
        + reticulum::crypt::fernet::FERNET_OVERHEAD_SIZE
        + reticulum::crypt::fernet::FERNET_MAX_PADDING_SIZE;
    let mut out_buf = vec![0u8; out_len];

    let token = fernet.encrypt(plaintext.as_slice().into(), &mut out_buf).unwrap();

    assert_eq!(token.as_bytes(), expected.as_slice());
}
