    #[test]
    fn python_status_exposes_peer_peering_key_status() {
        let local_hash = [2u8; 16];
        let ready_peer = hex::encode([3u8; 16]);
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode(local_hash),
        );
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": "peer-unconfigured-key" })),
            })
            .expect("create unconfigured peer");
        daemon
            .accept_announce_with_metadata(
                ready_peer.clone(),
                1_700_000_620,
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
            .expect("accept propagation peer announce");
        daemon
            .accept_announce_with_metadata(
                "peer-not-ready-key".to_string(),
                1_700_000_621,
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
            .expect("accept invalid-hash propagation peer announce");

        let status = status::compose_python_status(
            &daemon,
            &PropagationControlContext {
                enabled: true,
                local_identity_hash: local_hash,
                propagation_destination_hash_hex: Some("propagation".to_string()),
                control_destination_hash_hex: Some("control".to_string()),
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: test_validated_peer_links(),
            },
        );

        assert_eq!(
            status["peers"]["peer-unconfigured-key"]["peering_key_status"].as_str(),
            Some("unconfigured")
        );
        assert_eq!(
            status["peers"][ready_peer.as_str()]["peering_key_status"].as_str(),
            Some("ready")
        );
        assert_eq!(
            status["peers"]["peer-not-ready-key"]["peering_key_status"].as_str(),
            Some("not_ready")
        );
    }

    #[test]
    fn python_status_prefers_peer_propagation_stamp_policy() {
        let peer = "peer-policy".to_string();
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "autopeer": true,
                    "target_cost": 16,
                    "stamp_cost_flexibility": 7,
                    "peering_cost": 18,
                })),
            })
            .expect("enable propagation");
        let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
            rmpv::Value::Boolean(false),
            rmpv::Value::from(1_700_000_700),
            rmpv::Value::Boolean(true),
            rmpv::Value::from(512),
            rmpv::Value::from(2048),
            rmpv::Value::Array(vec![
                rmpv::Value::from(4),
                rmpv::Value::from(1),
                rmpv::Value::from(6),
            ]),
            rmpv::Value::Map(Vec::new()),
        ]))
        .expect("encode propagation app data");
        daemon
            .accept_announce_with_metadata(
                peer.clone(),
                1_700_000_700,
                Some("Peer Policy".to_string()),
                Some("announce".to_string()),
                Some(hex::encode(app_data)),
                Some(vec!["propagation".to_string()]),
                None,
                None,
                None,
                Some(4),
                Some(Some(1)),
                Some(Some(6)),
                None,
                Some(1),
                None,
                None,
                None,
                None,
            )
            .expect("accept propagation peer announce");

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
        assert_eq!(peer_status["peering_timebase"].as_i64(), Some(1_700_000_700));
        assert_eq!(peer_status["transfer_limit"].as_u64(), Some(512));
        assert_eq!(peer_status["sync_limit"].as_u64(), Some(2048));
        assert_eq!(peer_status["target_stamp_cost"].as_u64(), Some(4));
        assert_eq!(peer_status["stamp_cost_flexibility"].as_u64(), Some(1));
        assert_eq!(peer_status["peering_cost"].as_u64(), Some(6));
    }
