    #[test]
    fn peer_identified_event_records_identity_for_following_control_requests() {
        let control = test_control_context();
        let link_id = test_link_id();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        record_identified_peer(&control, link_id, remote_identity);

        let stored = control
            .identified_peer_links
            .lock()
            .expect("identified peer links")
            .get(&link_id)
            .copied();
        assert_eq!(
            stored.map(|identity| identity.address_hash),
            Some(remote_identity.address_hash)
        );
        assert!(
            !control
                .validated_peer_links
                .lock()
                .expect("validated peer links")
                .contains(&link_id),
            "identity discovery must not bypass later peering validation"
        );

        clear_validated_peer_link(&control, &link_id);
        assert!(!control
            .identified_peer_links
            .lock()
            .expect("identified peer links")
            .contains_key(&link_id));
    }
