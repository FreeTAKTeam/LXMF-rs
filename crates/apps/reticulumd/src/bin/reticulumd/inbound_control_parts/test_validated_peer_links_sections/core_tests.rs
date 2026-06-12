    use super::*;

    use reticulum_daemon::lxmf_stamps::generate_peering_key;

    use rns_rpc::MessagesStore;

    use serde_json::json;

    use std::collections::HashSet;

    fn test_validated_peer_links() -> Arc<Mutex<HashSet<AddressHash>>> {
        Arc::new(Mutex::new(HashSet::new()))
    }

    fn test_link_id() -> AddressHash {
        AddressHash::new([0xA5; 16])
    }

    fn test_control_context() -> PropagationControlContext {
        PropagationControlContext {
            enabled: true,
            local_identity_hash: [0u8; 16],
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
        }
    }

    fn ready_propagation_daemon() -> RpcDaemon {
        RpcDaemon::test_instance_with_identity(hex::encode([2u8; 16]))
    }

    fn make_ready_propagation_peer(daemon: &RpcDaemon, peer_seed: u8) -> String {
        let peer = hex::encode([peer_seed; 16]);
        daemon
            .accept_announce_with_metadata(
                peer.clone(),
                1_700_000_606 + i64::from(peer_seed),
                None,
                None,
                None,
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(1),
                Some(Some(1)),
                Some(Some(1)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept ready propagation peer announce");
        peer
    }

    fn control_request(path: &str, data: rmpv::Value) -> Vec<u8> {
        rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
            rmpv::Value::Nil,
            rmpv::Value::Binary(control_path_hash(path).to_vec()),
            data,
        ]))
        .expect("encode control request")
    }

    #[test]
    fn closed_link_clears_validated_peer_link_like_python() {
        let control = test_control_context();
        let link_id = test_link_id();
        control.validated_peer_links.lock().expect("validated peer links").insert(link_id);

        clear_validated_peer_link(&control, &link_id);

        assert!(!control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
    }

    #[test]
    fn stats_request_returns_nil_when_propagation_node_is_disabled() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        let response = handle_control_request(
            &daemon,
            &test_control_context(),
            &test_link_id(),
            control_request("/pn/get/stats", rmpv::Value::Nil).as_slice(),
            Some(&remote_identity),
            false,
        );

        assert!(matches!(response, ControlResponse::Value(Value::Null)));
    }

    #[test]
    fn stats_request_returns_status_when_propagation_node_is_enabled() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({ "enabled": true })),
            })
            .expect("enable propagation");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();

        let response = handle_control_request(
            &daemon,
            &test_control_context(),
            &test_link_id(),
            control_request("/pn/get/stats", rmpv::Value::Nil).as_slice(),
            Some(&remote_identity),
            false,
        );

        let ControlResponse::Value(status) = response else {
            panic!("expected status value");
        };
        assert_eq!(status["peers"].as_object().map(|peers| peers.len()), Some(0));
        assert_eq!(status["total_peers"].as_u64(), Some(0));
    }

    #[test]
    fn stats_request_rejects_identity_outside_control_allow_list() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({ "enabled": true })),
            })
            .expect("enable propagation");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let mut control = test_control_context();
        control.allowed_control_identities = vec!["not-the-remote".to_string()];

        let response = handle_control_request(
            &daemon,
            &control,
            &test_link_id(),
            control_request("/pn/get/stats", rmpv::Value::Nil).as_slice(),
            Some(&remote_identity),
            false,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
    }

    #[test]
    fn propagation_offer_ignores_control_allow_list_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
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
            allowed_control_identities: vec!["not-the-remote".to_string()],
            validated_peer_links: test_validated_peer_links(),
        };
        let response = handle_control_request(
            &daemon,
            &control,
            &test_link_id(),
            control_request(
                "/offer",
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(peering_key),
                    rmpv::Value::Array(Vec::new()),
                ]),
            )
            .as_slice(),
            Some(&remote_identity),
            true,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
    }

    #[test]
    fn python_status_uses_propagation_stamp_flexibility_not_delivery_stamp_flexibility() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "target_cost": 16,
                    "stamp_cost_flexibility": 7,
                    "peering_cost": 18,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "stamp_policy_set".to_string(),
                params: Some(json!({
                    "target_cost": 11,
                    "flexibility": 2,
                })),
            })
            .expect("set delivery stamp policy");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["stamp_cost_flexibility"].as_u64(), Some(7));
    }

    #[test]
    fn python_status_reports_elapsed_uptime_not_epoch_time() {
        let daemon = RpcDaemon::test_instance();

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert!(
            status["uptime"].as_u64().is_some_and(|value| value < 60),
            "uptime should be elapsed seconds, not Unix epoch seconds"
        );
    }

    #[test]
    fn python_status_uses_configured_node_transfer_limits() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "delivery_limit": 321,
                    "propagation_limit": 654,
                    "sync_limit": 987,
                })),
            })
            .expect("enable propagation");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["delivery_limit"].as_u64(), Some(321));
        assert_eq!(status["propagation_limit"].as_u64(), Some(654));
        assert_eq!(status["sync_limit"].as_u64(), Some(987));
    }

    #[test]
    fn python_status_reports_message_storage_limit_in_decimal_bytes() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "message_storage_limit_mb": 4,
                })),
            })
            .expect("enable propagation");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["messagestore"]["limit"].as_u64(), Some(4_000_000));
    }

    #[test]
    fn python_status_uses_zero_acceptance_rate_before_offers() {
        let peer = "peer-zero-acceptance".to_string();
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(status["peers"][peer.as_str()]["acceptance_rate"].as_f64(), Some(0.0));
    }

    #[test]
    fn python_status_reports_peer_sync_transfer_rate_counter() {
        let daemon = ready_propagation_daemon();
        let peer = make_ready_propagation_peer(&daemon, 0x91);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({ "enabled": true })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": "19".repeat(24) })),
            })
            .expect("ingest propagation");
        let sync = daemon
            .handle_rpc(RpcRequest {
                id: 3,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer, "transfer_limit_kb": 1 })),
            })
            .expect("peer sync")
            .result
            .expect("peer sync result");
        let transferred_bytes =
            sync["sync_transfer_rate"].as_f64().expect("sync transfer rate counter") as u64;
        assert!(transferred_bytes > 0);

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: [0u8; 16],
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["sync_transfer_rate"].as_f64(), Some(transferred_bytes as f64));
        assert_eq!(peer_status["str"].as_u64(), Some(transferred_bytes));
    }
