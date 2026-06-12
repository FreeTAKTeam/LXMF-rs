    #[test]
    fn python_status_reports_peer_propagation_message_ids() {
        let daemon = ready_propagation_daemon();
        let peer = make_ready_propagation_peer(&daemon, 0x92);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");
        let handled_id = "8a".repeat(32);
        let unhandled_id = "8b".repeat(32);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                b"handled propagation payload",
                handled_id.as_str(),
                &[],
            )
            .expect("store handled payload");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer, "transfer_limit_kb": 1 })),
            })
            .expect("handle first payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                b"unhandled propagation payload",
                unhandled_id.as_str(),
                &[],
            )
            .expect("store unhandled payload");
        daemon.record_propagation_offer_peer(peer.as_str()).expect("record offered peer");

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
        assert_eq!(
            peer_status["messages"]["handled_ids"].as_array().expect("message handled ids"),
            &[json!(handled_id.as_str())]
        );
        assert_eq!(
            peer_status["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
            &[json!(unhandled_id.as_str())]
        );
        assert_eq!(
            peer_status["handled_ids"].as_array().expect("top-level handled ids"),
            &[json!(handled_id.as_str())]
        );
        assert_eq!(
            peer_status["unhandled_ids"].as_array().expect("top-level unhandled ids"),
            &[json!(unhandled_id.as_str())]
        );
    }

    #[test]
    fn python_status_reports_peer_message_counters_at_top_level() {
        let daemon = ready_propagation_daemon();
        let peer = make_ready_propagation_peer(&daemon, 0x93);
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync");
        let handled_id = "8c".repeat(32);
        let handled_payload = [0x14; 32];
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                handled_payload.as_slice(),
                handled_id.as_str(),
                &[],
            )
            .expect("store handled propagation payload");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer, "transfer_limit_kb": 1 })),
            })
            .expect("handle first payload");
        let unhandled_id = "8d".repeat(32);
        let unhandled_payload = [0x15; 32];
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                unhandled_payload.as_slice(),
                unhandled_id.as_str(),
                &[],
            )
            .expect("store unhandled propagation payload");
        daemon.record_propagation_offer_peer(peer.as_str()).expect("record offered peer");

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
        assert_eq!(peer_status["messages"]["offered"].as_u64(), Some(1));
        assert_eq!(peer_status["messages"]["unhandled"].as_u64(), Some(1));
        assert_eq!(peer_status["messages"]["offered_bytes"].as_u64(), Some(32));
        assert_eq!(peer_status["messages"]["unhandled_bytes"].as_u64(), Some(32));
        assert_eq!(peer_status["offered"].as_u64(), Some(1));
        assert_eq!(peer_status["outgoing"].as_u64(), Some(1));
        assert_eq!(peer_status["incoming"].as_u64(), Some(0));
        assert_eq!(peer_status["unhandled"].as_u64(), Some(1));
        assert_eq!(peer_status["offered_bytes"].as_u64(), Some(32));
        assert_eq!(peer_status["unhandled_bytes"].as_u64(), Some(32));
    }

    #[test]
    fn python_status_reports_peer_record_metadata() {
        let peer = "peer-record-metadata".to_string();
        let daemon = RpcDaemon::test_instance();
        let sync = daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": peer })),
            })
            .expect("peer sync")
            .result
            .expect("peer sync result");
        let first_seen = sync["first_seen"].as_i64().expect("first_seen");
        assert!(first_seen > 0);

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
        assert_eq!(peer_status["peer_type"].as_str(), Some("manual"));
        assert_eq!(peer_status["first_seen"].as_i64(), Some(first_seen));
        assert_eq!(peer_status["seen_count"].as_u64(), Some(1));
        assert_eq!(peer_status["sync_strategy"].as_u64(), Some(2));
    }

    #[test]
    fn python_status_reports_propagation_node_runtime_state() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": ["peer-selected-node"],
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "set_outbound_propagation_node".to_string(),
                params: Some(json!({ "peer": "peer-selected-node" })),
            })
            .expect("set selected node");

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

        assert_eq!(status["selected_node"].as_str(), Some("peer-selected-node"));
        assert_eq!(status["sync_state"].as_u64(), Some(0));
        assert_eq!(status["sync_progress"].as_f64(), Some(0.0));
        assert_eq!(status["last_sync_started"], Value::Null);
        assert_eq!(status["last_sync_completed"], Value::Null);
        assert_eq!(status["last_sync_error"], Value::Null);
    }

    #[test]
    fn python_status_reports_propagation_policy_and_ingest_counters() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "autopeer": false,
                    "autopeer_maxdepth": 2,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_ingest".to_string(),
                params: Some(json!({ "payload_hex": "2a".repeat(24) })),
            })
            .expect("ingest propagation");

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

        assert_eq!(status["autopeer"].as_bool(), Some(false));
        assert_eq!(status["autopeer_maxdepth"].as_u64(), Some(2));
        assert_eq!(status["total_ingested"].as_u64(), Some(1));
        assert_eq!(status["last_ingest_count"].as_u64(), Some(1));
        assert_eq!(status["messages_received"].as_u64(), Some(0));
        assert_eq!(status["max_messages"].as_u64(), Some(0));
    }

    #[test]
    fn python_status_preserves_unknown_peer_propagation_policy_as_null() {
        let peer = "peer-unknown-policy".to_string();
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
                    "propagation_limit": 654,
                    "sync_limit": 987,
                })),
            })
            .expect("enable propagation");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
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

        let peer_status = &status["peers"][peer.as_str()];
        assert_eq!(peer_status["transfer_limit"], Value::Null);
        assert_eq!(peer_status["sync_limit"], Value::Null);
        assert_eq!(peer_status["target_stamp_cost"], Value::Null);
        assert_eq!(peer_status["stamp_cost_flexibility"], Value::Null);
        assert_eq!(peer_status["peering_cost"], Value::Null);
    }

    #[test]
    fn python_status_collapses_internal_peer_types_to_static_or_discovered() {
        let static_peer = "peer-static".to_string();
        let auto_peer = "peer-auto".to_string();
        let manual_peer = "peer-manual".to_string();
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": [static_peer],
                    "autopeer": true,
                })),
            })
            .expect("enable propagation");
        daemon
            .accept_announce_with_metadata(
                auto_peer.clone(),
                1_700_000_800,
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
            .expect("accept auto peer announce");
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "peer_sync".to_string(),
                params: Some(json!({ "peer": manual_peer })),
            })
            .expect("manual peer sync");

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

        assert_eq!(status["peers"][static_peer.as_str()]["type"].as_str(), Some("static"));
        assert_eq!(status["peers"][auto_peer.as_str()]["type"].as_str(), Some("discovered"));
        assert_eq!(status["peers"][manual_peer.as_str()]["type"].as_str(), Some("discovered"));
        assert_eq!(status["static_peers"].as_u64(), Some(1));
        assert_eq!(status["discovered_peers"].as_u64(), Some(2));
        assert_eq!(status["total_peers"].as_u64(), Some(3));
    }

    #[test]
    fn python_status_exposes_peer_peering_key_value() {
        let local_hash = [2u8; 16];
        let peer = hex::encode([3u8; 16]);
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode(local_hash),
        );
        daemon
            .accept_announce_with_metadata(
                peer.clone(),
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

        assert!(status["peers"][peer.as_str()]["peering_key"]
            .as_u64()
            .is_some_and(|value| value >= 1));
    }
