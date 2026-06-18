    #[test]
    fn offer_request_invalid_peering_key_starts_offer_throttle_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 255,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let offered = [0xBB; 32];
        let other_offered = [0xBC; 32];
        let invalid_peering_key = vec![0x00; 1];
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let first = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(invalid_peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );
        let second = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(invalid_peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(other_offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(first, ControlResponse::Code(0xF3)));
        assert!(matches!(second, ControlResponse::Code(0xF6)));
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let peers = daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "list_propagation_nodes".to_string(),
                params: None,
            })
            .expect("list propagation nodes")
            .result
            .expect("peer rows");
        let peer_rows = peers["nodes"].as_array().expect("peer rows");
        assert!(
            !peer_rows
                .iter()
                .any(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str())),
            "invalid peering keys should not admit the peer"
        );
        assert!(
            control.validated_peer_links.lock().expect("validated peer links").is_empty(),
            "invalid peering keys should not validate the offer link"
        );
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    hex::encode(offered).as_str(),
                )
                .expect("completed mark"),
            "invalid peering keys should not create peer queue marks"
        );
    }

    #[test]
    fn offer_request_short_array_returns_nil_without_recording_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let link_id = test_link_id();
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Binary(vec![0xAA; 32])])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Rmpv(rmpv::Value::Nil)));
        assert!(!control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "short offer request must not create a peer record"
        );
    }

    #[test]
    fn offer_request_rejects_invalid_transient_id_without_recording_peer() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(vec![0xAA; 31])]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF4)));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "invalid offer data must not create a peer record"
        );
    }

    #[test]
    fn offer_request_rejects_mixed_invalid_offer_without_partial_queue_marks() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let known_payload = b"known before invalid offer id";
        let known_transient_id = hex::encode(sha2::Sha256::digest(known_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                known_payload,
                known_transient_id.as_str(),
                &[],
            )
            .expect("store known payload");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let link_id = test_link_id();
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(
                        hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                    ),
                    rmpv::Value::Binary(vec![0xAA; 31]),
                ]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF4)));
        assert!(!control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    known_transient_id.as_str(),
                )
                .expect("known offer mark"),
            "invalid offer data must not leave partial source-accounting queue marks"
        );
    }

    #[test]
    fn offer_request_deduplicates_missing_wanted_ids_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let known_payload = b"known before duplicate missing offers";
        let known_transient_id = hex::encode(sha2::Sha256::digest(known_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                known_payload,
                known_transient_id.as_str(),
                &[],
            )
            .expect("store known payload");
        let missing_transient = [0x64; 32];
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(
                        hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                    ),
                    rmpv::Value::Binary(
                        hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                    ),
                    rmpv::Value::Binary(missing_transient.to_vec()),
                    rmpv::Value::Binary(missing_transient.to_vec()),
                ]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(wanted)) = response else {
            panic!("expected partial wanted-id list");
        };
        assert_eq!(
            wanted,
            vec![rmpv::Value::Binary(missing_transient.to_vec())],
            "duplicate offered missing IDs should be requested once"
        );
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    known_transient_id.as_str(),
                )
                .expect("known offer mark"),
            "duplicate known offered IDs should be accounted once as source-completed"
        );
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("admit peer after duplicate offer");
        let row = list_peer_row(&daemon, remote_propagation_hash.as_str());
        assert_eq!(row["messages"]["handled_ids"], json!([known_transient_id]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }
