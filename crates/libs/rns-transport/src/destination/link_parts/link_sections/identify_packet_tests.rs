// Split into its own file (rather than inline in `new.rs`) to keep that
// module under the 500 LOC policy — mirrors this crate's own established
// pattern for `link_status_predicates_make_lifecycl_sections/*.rs`. A
// distinct module name (not `tests`) since `link_status_predicates_make_
// lifecycl.rs` already declares one `#[cfg(test)] mod tests`; this keeps
// the restoration self-contained rather than also touching that unrelated
// include chain. See #476.
#[cfg(test)]
mod identify_packet_tests {
    use super::*;

    fn build_test_identify_payload(private: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
        let identity = private.as_identity();
        let mut payload = Vec::with_capacity(PUBLIC_KEY_LENGTH * 2 + SIGNATURE_LENGTH);
        payload.extend_from_slice(identity.public_key_bytes());
        payload.extend_from_slice(identity.verifying_key_bytes());
        let mut signed = Vec::with_capacity(ADDRESS_HASH_SIZE + PUBLIC_KEY_LENGTH * 2);
        signed.extend_from_slice(link_id.as_slice());
        signed.extend_from_slice(identity.public_key_bytes());
        signed.extend_from_slice(identity.verifying_key_bytes());
        payload.extend_from_slice(&private.sign(&signed).to_bytes());
        payload
    }

    #[test]
    fn parse_link_identify_accepts_valid_proof() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let payload = build_test_identify_payload(&private, &link_id);
        let result = parse_link_identify_payload(&payload, &link_id);
        assert_eq!(result.map(|i| i.address_hash), Some(private.as_identity().address_hash));
    }

    #[test]
    fn parse_link_identify_rejects_short_payload() {
        let link_id = AddressHash::new([0x01; ADDRESS_HASH_SIZE]);
        assert!(parse_link_identify_payload(&[0u8; 64], &link_id).is_none());
    }

    #[test]
    fn parse_link_identify_rejects_trailing_bytes() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let mut payload = build_test_identify_payload(&private, &link_id);
        payload.push(0x00);
        assert!(parse_link_identify_payload(&payload, &link_id).is_none());
    }

    #[test]
    fn parse_link_identify_rejects_corrupted_signature() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let mut payload = build_test_identify_payload(&private, &link_id);
        payload[PUBLIC_KEY_LENGTH * 2] ^= 0xFF;
        assert!(parse_link_identify_payload(&payload, &link_id).is_none());
    }

    #[test]
    fn parse_link_identify_rejects_wrong_link_id() {
        let link_id_a = AddressHash::new([0xAA; ADDRESS_HASH_SIZE]);
        let link_id_b = AddressHash::new([0xBB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);
        let payload = build_test_identify_payload(&private, &link_id_a);
        assert!(parse_link_identify_payload(&payload, &link_id_b).is_none());
    }
}
