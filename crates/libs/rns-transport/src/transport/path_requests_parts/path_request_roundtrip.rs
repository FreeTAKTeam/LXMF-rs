#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_request_roundtrip() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);

        let dest = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);

        let encoded = testee.generate(&dest, None);
        let decoded = testee.decode(encoded.data.as_slice(), iface).unwrap();

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
        let iface = AddressHash::new_from_rand(OsRng);
        let tag = vec![0x55; ADDRESS_HASH_SIZE];
        let packet = testee.generate(&destination, Some(tag));
        let now = Instant::now();

        assert!(testee.decode_at(packet.data.as_slice(), iface, now).is_some());
        assert!(testee.decode_at(packet.data.as_slice(), iface, now).is_none());

        assert!(testee
            .decode_at(packet.data.as_slice(), iface, now + Duration::from_millis(1100))
            .is_some());
    }

    #[test]
    fn duplicate_path_requests_use_exact_destination_and_tag_key() {
        let mut receiver = PathRequests::new("", None, 16, 16, 30);
        let requester = AddressHash::new_from_rand(OsRng);
        let mut sender = PathRequests::new("", Some(requester), 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);
        let tag = vec![0x56; ADDRESS_HASH_SIZE];
        let packet = sender.generate(&destination, Some(tag));
        let now = Instant::now();

        let decoded = receiver
            .decode_at(packet.data.as_slice(), iface, now)
            .expect("first request should be accepted");
        assert_eq!(decoded.requesting_transport, Some(requester));
        assert!(receiver.decode_at(packet.data.as_slice(), iface, now).is_none());
    }

    #[test]
    fn duplicate_path_requests_suppress_replays_across_requesters_and_interfaces() {
        let mut receiver = PathRequests::new("", None, 16, 16, 30);
        let requester_a = AddressHash::new_from_rand(OsRng);
        let requester_b = AddressHash::new_from_rand(OsRng);
        let mut sender_a = PathRequests::new("", Some(requester_a), 16, 16, 30);
        let mut sender_b = PathRequests::new("", Some(requester_b), 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);
        let tag = vec![0x57; ADDRESS_HASH_SIZE];
        let packet_a = sender_a.generate(&destination, Some(tag.clone()));
        let packet_b = sender_b.generate(&destination, Some(tag));
        let now = Instant::now();

        assert!(receiver.decode_at(packet_a.data.as_slice(), iface_a, now).is_some());
        assert!(
            receiver.decode_at(packet_b.data.as_slice(), iface_a, now).is_none(),
            "same destination/tag must be suppressed for a distinct requester"
        );
        assert!(
            receiver.decode_at(packet_a.data.as_slice(), iface_b, now).is_none(),
            "same destination/tag must be suppressed on a distinct interface"
        );
        assert!(
            receiver.decode_at(packet_a.data.as_slice(), iface_a, now).is_none(),
            "same destination/tag/requester/iface should still be suppressed"
        );
    }

    #[test]
    fn rns_1_5_path_request_batch_coalesces_destination_and_retains_interfaces() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);

        assert!(testee.generate_recursive(&destination, Some(iface_a), None).is_some());
        assert!(testee.generate_recursive(&destination, Some(iface_a), None).is_none());
        assert!(testee.generate_recursive(&destination, Some(iface_b), None).is_none());
        assert_eq!(testee.take_discovery_requesters(&destination), vec![iface_a, iface_b]);
        assert!(testee.generate_recursive(&destination, Some(iface_b), None).is_some());
    }

    #[test]
    fn rns_1_5_discovery_timeout_uses_slow_medium_lower_bound() {
        let mut testee = PathRequests::new("", None, 16, 16, 15);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);
        let now = Instant::now();

        testee.set_request_timeout_lower_bound(Duration::from_secs(90));
        assert!(testee.allow_recursive_at(&destination, Some(iface), now));
        assert!(!testee.allow_recursive_at(
            &destination,
            Some(iface),
            now + Duration::from_secs(89)
        ));
        assert!(testee.allow_recursive_at(
            &destination,
            Some(iface),
            now + Duration::from_secs(91)
        ));
    }

    #[test]
    fn rns_1_5_discovery_timeout_tracks_the_current_medium_lower_bound() {
        let mut testee = PathRequests::new("", None, 16, 16, 15);

        testee.set_request_timeout_lower_bound(Duration::from_secs(90));
        assert_eq!(testee.request_timeout, Duration::from_secs(90));

        testee.set_request_timeout_lower_bound(Duration::from_secs(30));
        assert_eq!(testee.request_timeout, Duration::from_secs(30));

        testee.set_request_timeout_lower_bound(Duration::from_secs(5));
        assert_eq!(testee.request_timeout, Duration::from_secs(15));
    }

    #[test]
    fn rns_1_5_prequeue_discovery_expiry_rebases_when_slow_request_engages() {
        let mut testee = PathRequests::new("", None, 16, 16, 15);
        let destination = AddressHash::new_from_rand(OsRng);
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(testee.register_discovery_before_queue(&destination, iface));
        let prequeue_expiry = testee
            .discovery
            .get(&destination)
            .expect("prequeue discovery")
            .expires_at;

        testee.set_request_timeout_lower_bound(Duration::from_secs(90));
        let engaged_at = Instant::now();
        assert!(testee.allow_recursive_at(&destination, Some(iface), engaged_at));
        let engaged = testee.discovery.get(&destination).expect("engaged discovery");
        assert!(engaged.engaged);
        assert!(engaged.expires_at >= engaged_at + Duration::from_secs(90));
        assert!(engaged.expires_at > prequeue_expiry);
    }

    #[test]
    fn matching_announce_consumes_waiting_discovery_requesters() {
        let mut testee = PathRequests::new("", None, 1, 1, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let other_destination = AddressHash::new_from_rand(OsRng);
        let iface_a = AddressHash::new_from_rand(OsRng);
        let iface_b = AddressHash::new_from_rand(OsRng);

        assert!(testee.generate_recursive(&destination, Some(iface_a), None).is_some());
        assert!(testee.generate_recursive(&other_destination, Some(iface_a), None).is_none());

        assert_eq!(testee.take_discovery_requesters(&destination), vec![iface_a]);
        assert!(testee.generate_recursive(&other_destination, Some(iface_a), None).is_some());
        assert_eq!(testee.take_discovery_requesters(&other_destination), vec![iface_a]);
        assert!(testee.take_discovery_requesters(&destination).is_empty());

        assert!(testee.generate_recursive(&destination, Some(iface_b), None).is_some());
        assert_eq!(testee.take_discovery_requesters(&destination), vec![iface_b]);
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
    fn outgoing_path_requests_are_throttled_like_python() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let cooldown = Duration::from_secs(20);
        let now = Instant::now();

        assert!(!testee.outgoing_request_recently_sent(&destination, now, cooldown));
        testee.record_outgoing_request_at(&destination, now);
        assert!(testee.outgoing_request_recently_sent(
            &destination,
            now + Duration::from_secs(19),
            cooldown
        ));
        assert!(!testee.outgoing_request_recently_sent(
            &destination,
            now + cooldown + Duration::from_millis(1),
            cooldown
        ));
    }

    #[test]
    fn refreshing_outgoing_path_request_survives_stale_queue_entry() {
        let mut testee = PathRequests::new("", None, 16, 16, 30);
        let destination = AddressHash::new_from_rand(OsRng);
        let cooldown = Duration::from_secs(20);
        let now = Instant::now();
        let refresh_at = now + cooldown + Duration::from_millis(1);

        testee.record_outgoing_request_at(&destination, now);
        testee.record_outgoing_request_at(&destination, refresh_at);

        assert!(testee.outgoing_request_recently_sent(
            &destination,
            refresh_at + Duration::from_millis(1),
            cooldown
        ));
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
