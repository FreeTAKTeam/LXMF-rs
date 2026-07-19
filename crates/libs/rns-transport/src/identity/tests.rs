use super::{Identity, PrivateIdentity, PUBLIC_KEY_LENGTH};
use rand_core::OsRng;

#[test]
fn private_identity_hex_string() {
    let original_id = PrivateIdentity::new_from_rand(OsRng);
    let original_hex = original_id.to_hex_string();

    let actual_id = PrivateIdentity::new_from_hex_string(&original_hex).expect("valid identity");

    assert_eq!(actual_id.private_key.as_bytes(), original_id.private_key.as_bytes());
    assert_eq!(actual_id.sign_key.as_bytes(), original_id.sign_key.as_bytes());
}

#[test]
fn public_identity_constructors_reject_malformed_keys() {
    let valid = PrivateIdentity::new_from_rand(OsRng);
    let identity = valid.as_identity();
    assert!(Identity::try_new_from_slices(
        identity.public_key_bytes(),
        identity.verifying_key_bytes()
    )
    .is_ok());
    assert!(Identity::try_new_from_slices(&[0_u8; PUBLIC_KEY_LENGTH - 1], &[0_u8; 32]).is_err());
    let invalid_verifying_key = (0_u8..=u8::MAX)
        .map(|byte| [byte; PUBLIC_KEY_LENGTH])
        .find(|bytes| ed25519_dalek::VerifyingKey::from_bytes(bytes).is_err())
        .expect("at least one compressed point encoding must be invalid");
    assert!(Identity::try_new_from_slices(&[0_u8; 32], &invalid_verifying_key).is_err());
    assert!(Identity::new_from_hex_string(&"zz".repeat(PUBLIC_KEY_LENGTH * 2)).is_err());
    assert!(Identity::new_from_hex_string(&"00".repeat(PUBLIC_KEY_LENGTH * 2 + 1)).is_err());
}

#[test]
fn private_identity_hex_constructor_rejects_malformed_keys() {
    assert!(PrivateIdentity::new_from_hex_string("not-an-identity").is_err());
    assert!(PrivateIdentity::new_from_hex_string(&"00".repeat(PUBLIC_KEY_LENGTH * 2 - 1)).is_err());
    assert!(PrivateIdentity::new_from_hex_string(&"00".repeat(PUBLIC_KEY_LENGTH * 2 + 1)).is_err());
}
