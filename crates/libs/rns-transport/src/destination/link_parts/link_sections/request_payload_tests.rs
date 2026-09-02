// Kept separate from `request_payload.rs` so each stays below the
// repository's 500-line limit. `include!`d into the same module.
#[cfg(test)]
mod request_payload_tests {
    use super::*;

    use crate::destination::{DestinationDesc, DestinationName};

    fn decode(packed: &[u8]) -> rmpv::Value {
        rmpv::decode::read_value(&mut std::io::Cursor::new(packed)).expect("decodes")
    }

    #[test]
    fn a_request_payload_is_the_reference_three_element_array() {
        let data = rmpv::Value::Map(vec![(rmpv::Value::from("field_name"), rmpv::Value::from("value"))]);
        let request = Link::request_payload("/page/index.mu", data.clone()).expect("packs");

        let value = decode(&request.packed);
        let items = value.as_array().expect("an array");
        assert_eq!(items.len(), 3);
        assert!(items[0].as_f64().is_some_and(|time| time > 1_700_000_000.0), "a `time.time()` timestamp");
        assert_eq!(items[1].as_slice(), Some(&crate::hash::address_hash(b"/page/index.mu")[..]));
        assert_eq!(items[1].as_slice(), Some(&request.path_hash[..]));
        assert_eq!(items[2], data, "the body keeps its own msgpack type");
        assert_eq!(request.resource_request_id, crate::hash::address_hash(&request.packed));
    }

    #[test]
    fn a_request_with_no_body_packs_nil_not_an_empty_byte_string() {
        let request = Link::request_payload("/page/index.mu", rmpv::Value::Nil).expect("packs");

        let value = decode(&request.packed);
        assert_eq!(value.as_array().and_then(|items| items.get(2)), Some(&rmpv::Value::Nil));
    }

    #[test]
    fn an_identify_payload_is_what_the_receiving_side_verifies() {
        let link_id = AddressHash::new([0xAB; ADDRESS_HASH_SIZE]);
        let private = PrivateIdentity::new_from_rand(OsRng);

        let payload = identify_payload(&private, &link_id);

        assert_eq!(payload.len(), LINK_IDENTIFY_PAYLOAD_LENGTH);
        assert_eq!(
            parse_link_identify_payload(&payload, &link_id).map(|identity| identity.address_hash),
            Some(private.as_identity().address_hash)
        );
        let other_link = AddressHash::new([0x00; ADDRESS_HASH_SIZE]);
        assert!(parse_link_identify_payload(&payload, &other_link).is_none(), "bound to the link it was made for");
    }

    #[test]
    fn a_links_identify_payload_is_bound_to_its_own_id() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (tx, _rx) = tokio::sync::broadcast::channel(8);
        let link = Link::new(destination, tx);

        let payload = link.identify_payload(&signer);

        assert_eq!(
            parse_link_identify_payload(&payload, link.id()).map(|identity| identity.address_hash),
            Some(identity.address_hash)
        );
    }

    #[test]
    fn a_response_envelope_unpacks_to_its_request_id_and_response() {
        let mut packed = Vec::new();
        rmpv::encode::write_value(
            &mut packed,
            &rmpv::Value::Array(vec![
                rmpv::Value::Binary(vec![0x42; ADDRESS_HASH_SIZE]),
                rmpv::Value::Binary(b"hello".to_vec()),
            ]),
        )
        .expect("packs");

        let (request_id, response) = unpack_response_envelope(&packed).expect("unpacks");

        assert_eq!(request_id, [0x42; ADDRESS_HASH_SIZE]);
        assert_eq!(response.as_slice(), Some(&b"hello"[..]));
    }

    #[test]
    fn a_response_envelope_of_the_wrong_shape_is_refused() {
        let mut trailing = Vec::new();
        rmpv::encode::write_value(
            &mut trailing,
            &rmpv::Value::Array(vec![
                rmpv::Value::Binary(vec![0x42; ADDRESS_HASH_SIZE]),
                rmpv::Value::Binary(b"hello".to_vec()),
            ]),
        )
        .expect("packs");
        let mut three = Vec::new();
        rmpv::encode::write_value(&mut three, &rmpv::Value::Array(vec![rmpv::Value::Nil; 3])).expect("packs");
        let mut short_id = Vec::new();
        rmpv::encode::write_value(
            &mut short_id,
            &rmpv::Value::Array(vec![rmpv::Value::Binary(vec![0x42; 4]), rmpv::Value::Nil]),
        )
        .expect("packs");
        trailing.push(0x00);

        for (label, bytes) in [("trailing bytes", trailing), ("three elements", three), ("a short id", short_id)] {
            assert!(unpack_response_envelope(&bytes).is_err(), "{label} must be refused");
        }
    }
}
