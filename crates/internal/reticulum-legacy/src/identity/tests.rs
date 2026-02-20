#[cfg(test)]
mod tests {
    use rand_core::OsRng;

    use super::PrivateIdentity;

    #[test]
    fn private_identity_hex_string() {
        let original_id = PrivateIdentity::new_from_rand(OsRng);
        let original_hex = original_id.to_hex_string();

        let actual_id =
            PrivateIdentity::new_from_hex_string(&original_hex).expect("valid identity");

        assert_eq!(actual_id.private_key.as_bytes(), original_id.private_key.as_bytes());

        assert_eq!(actual_id.sign_key.as_bytes(), original_id.sign_key.as_bytes());
    }
}
