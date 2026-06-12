#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_request_roundtrip() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);

        let dest = AddressHash::new_from_rand(OsRng);

        let encoded = testee.generate(&dest, None);
        let decoded = testee.decode(encoded.data.as_slice()).unwrap();

        assert_eq!(decoded.destination, dest);
    }

    #[test]
    fn recursive_path_request_preserves_supplied_tag() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);
        let tag = vec![0xAA; ADDRESS_HASH_SIZE];

        let packet = testee
            .generate_recursive(&destination, Some(iface), Some(tag.clone()))
            .expect("recursive request");
        let decoded = PathRequest::decode(packet.data.as_slice(), "").expect("decode request");

        assert_eq!(decoded.tag_bytes, tag);
    }

    #[test]
    fn duplicate_path_request_entries_expire() {
        let mut testee = PathRequests::new("", None, 16, 16, 1);
        let destination = AddressHash::new_from_rand(OsRng);
        let tag = vec![0x55; ADDRESS_HASH_SIZE];
        let packet = testee.generate(&destination, Some(tag));
        let now = Instant::now();

        assert!(testee.decode_at(packet.data.as_slice(), now).is_some());
        assert!(testee.decode_at(packet.data.as_slice(), now).is_none());

        assert!(testee
            .decode_at(packet.data.as_slice(), now + Duration::from_millis(1100))
            .is_some());
    }

    #[test]
    fn recursive_requests_are_tracked_per_interface() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);

        assert!(testee.generate_recursive(&destination, Some(iface_a), None).is_some());
        assert!(testee.generate_recursive(&destination, Some(iface_a), None).is_none());
        assert!(testee.generate_recursive(&destination, Some(iface_b), None).is_some());
    }

    #[test]
    fn local_responses_are_throttled_per_interface() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);
        let requester = Some(AddressHash::new_from_rand(OsRng));
        let now = Instant::now();

        assert!(testee.allow_local_response_at(&destination, requester, b"tag-a", iface_a, now));
        assert!(!testee.allow_local_response_at(&destination, requester, b"tag-a", iface_a, now));
        assert!(testee.allow_local_response_at(&destination, requester, b"tag-a", iface_b, now));
    }

    #[test]
    fn local_response_throttle_expires_after_cooldown() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);
        let requester = Some(AddressHash::new_from_rand(OsRng));
        let now = Instant::now();

        assert!(testee.allow_local_response_at(&destination, requester, b"tag-a", iface, now));
        assert!(!testee.allow_local_response_at(&destination, requester, b"tag-a", iface, now));
        assert!(testee.allow_local_response_at(
            &destination,
            requester,
            b"tag-a",
            iface,
            now + super::super::LOCAL_PATH_RESPONSE_COOLDOWN + Duration::from_millis(1)
        ));
    }

    #[test]
    fn local_response_throttle_is_scoped_per_requesting_transport() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let requester_a = Some(AddressHash::new_from_rand(OsRng));
        let requester_b = Some(AddressHash::new_from_rand(OsRng));
        let iface = AddressHash::new_from_rand(OsRng);
        let now = Instant::now();

        assert!(testee.allow_local_response_at(&destination, requester_a, b"tag-a", iface, now));
        assert!(testee.allow_local_response_at(&destination, requester_b, b"tag-a", iface, now));
        assert!(!testee.allow_local_response_at(&destination, requester_a, b"tag-a", iface, now));
    }

    #[test]
    fn local_response_throttle_is_scoped_per_request_tag() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let requester = Some(AddressHash::new_from_rand(OsRng));
        let iface = AddressHash::new_from_rand(OsRng);
        let now = Instant::now();

        assert!(testee.allow_local_response_at(&destination, requester, b"tag-a", iface, now));
        assert!(testee.allow_local_response_at(&destination, requester, b"tag-b", iface, now));
        assert!(!testee.allow_local_response_at(&destination, requester, b"tag-a", iface, now));
    }

    #[test]
    fn refreshing_an_expired_local_response_does_not_drop_the_new_entry() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let requester = Some(AddressHash::new_from_rand(OsRng));
        let iface = AddressHash::new_from_rand(OsRng);
        let cooldown = super::super::LOCAL_PATH_RESPONSE_COOLDOWN;
        let now = Instant::now();

        assert!(testee.allow_local_response_at(&destination, requester, b"tag-a", iface, now));
        let refresh_at = now + cooldown + Duration::from_millis(1);
        assert!(testee.allow_local_response_at(
            &destination,
            requester,
            b"tag-a",
            iface,
            refresh_at
        ));
        assert!(
            !testee.allow_local_response_at(
                &destination,
                requester,
                b"tag-a",
                iface,
                refresh_at + Duration::from_millis(1)
            ),
            "stale queue entries must not evict the refreshed cooldown"
        );
    }

    #[test]
    fn recursive_request_caps_are_scoped_per_interface() {
        let mut testee = PathRequests::new("", None, 16, 1, 30);
        let destination_a = AddressHash::new_from_rand(OsRng);
        let destination_b = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);

        assert!(testee.generate_recursive(&destination_a, Some(iface_a), None).is_some());
        assert!(testee.generate_recursive(&destination_b, Some(iface_a), None).is_none());
        assert!(testee.generate_recursive(&destination_b, Some(iface_b), None).is_some());
    }

    #[test]
    fn recursive_request_queue_limit_is_scoped_per_interface() {
        let mut testee = PathRequests::new("", None, 1, 0, 30);
        let destination_a = AddressHash::new_from_rand(OsRng);
        let destination_b = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);

        assert!(testee.generate_recursive(&destination_a, Some(iface_a), None).is_some());
        assert!(testee.generate_recursive(&destination_b, Some(iface_a), None).is_none());
        assert!(testee.generate_recursive(&destination_b, Some(iface_b), None).is_some());
    }

    #[test]
    fn expired_recursive_requests_release_interface_capacity() {
        let mut testee = PathRequests::new("", None, 1, 1, 1);
        let destination_a = AddressHash::new_from_rand(OsRng);
        let destination_b = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);
        let now = Instant::now();

        assert!(testee.allow_recursive_at(&destination_a, Some(iface), now));
        assert!(!testee.allow_recursive_at(
            &destination_b,
            Some(iface),
            now + Duration::from_millis(500)
        ));
        assert!(testee.allow_recursive_at(
            &destination_b,
            Some(iface),
            now + Duration::from_millis(1100)
        ));
    }
}
