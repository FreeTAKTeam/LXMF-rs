    #[test]
    fn offer_request_defers_capacity_limited_peer_admission_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                    "max_peers": 1,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-capacity-existing" })),
            })
            .expect("fill peer capacity");

        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let offered = [0xBB; 32];
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
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        let link_id = test_link_id();

        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(
            matches!(response, ControlResponse::Rmpv(rmpv::Value::Array(values)) if values.is_empty())
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "wanted offer response should not consume peer capacity before transfer admission"
        );
        assert!(
            control.validated_peer_links.lock().expect("validated peer links").contains(&link_id),
            "valid wanted offer should still validate the peering link"
        );
    }

    #[test]
    fn offer_request_capacity_limited_valid_offer_starts_throttle_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                    "max_peers": 1,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-capacity-existing" })),
            })
            .expect("fill peer capacity");

        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let offered = [0xBC; 32];
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
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let offer_data = || {
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ]))
        };
        let first = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            offer_data(),
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
            offer_data(),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(
            matches!(first, ControlResponse::Rmpv(rmpv::Value::Array(values)) if values.is_empty())
        );
        assert!(matches!(second, ControlResponse::Code(0xF6)));
    }

    #[test]
    fn offer_request_rejects_non_static_peer_when_static_only() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 11,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
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
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(Vec::new()),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn offer_request_allows_static_peer_destination_hash_when_static_only() {
        let daemon = RpcDaemon::test_instance();
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": [remote_propagation_hash],
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
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
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(Vec::new()),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
    }

    #[test]
    fn offer_request_static_only_rejects_before_data_validation() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Nil),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn message_get_rejects_identity_when_delivery_auth_required() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "auth_required": true,
                    "allowed_destinations": ["not-this-identity"],
                })),
            })
            .expect("set delivery policy");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn message_get_allows_python_identity_hash_when_delivery_auth_required() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        daemon
            .handle_rpc(RpcRequest {
                id: 13,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "auth_required": true,
                    "allowed_destinations": [hex::encode(remote_identity.address_hash.as_slice())],
                })),
            })
            .expect("set delivery policy");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(
            matches!(response, ControlResponse::Rmpv(rmpv::Value::Array(values)) if values.is_empty())
        );
    }

    #[test]
    fn message_get_rejects_delivery_destination_hash_when_auth_requires_python_identity_hash() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        daemon
            .handle_rpc(RpcRequest {
                id: 14,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "auth_required": true,
                    "allowed_destinations": [
                        hex::encode(delivery_destination_hash_for_identity(&remote_identity))
                    ],
                })),
            })
            .expect("set delivery policy");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }
