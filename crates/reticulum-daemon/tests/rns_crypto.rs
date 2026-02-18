use rand_core::OsRng;
use reticulum_daemon::rns_crypto::{decrypt_with_private_key, encrypt_for_public_key};
use x25519_dalek::{PublicKey, StaticSecret};

#[test]
fn ratchet_encrypt_roundtrip() {
    let private_key = StaticSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&private_key);
    let salt = [7u8; 16];
    let plaintext = b"hello ratchet";

    let ciphertext = encrypt_for_public_key(&public_key, &salt, plaintext, OsRng).expect("encrypt");
    let decrypted = decrypt_with_private_key(&private_key, &salt, &ciphertext).expect("decrypt");

    assert_eq!(decrypted, plaintext);
}
