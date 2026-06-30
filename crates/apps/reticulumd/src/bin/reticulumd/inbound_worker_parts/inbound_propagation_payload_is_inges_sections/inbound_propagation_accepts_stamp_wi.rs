    #[tokio::test]
    async fn inbound_propagation_accepts_stamp_within_flexibility_window() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 41,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 3,
                    "stamp_cost_flexibility": 2,
                })),
            })
            .expect("enable propagation");
        let lxm_data = [0x43_u8; 113];
        let transient = stamped_propagation_payload_with_value_range(&lxm_data, 1, 3);
        let transient_id = hex::encode(Sha256::digest(lxm_data));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![transient])).expect("propagation envelope");

        let ingested =
            ingest_propagation_envelope(&daemon, &envelope, None).await.expect("ingest envelope");
        assert_eq!(ingested, 1);

        let fetched = daemon
            .handle_rpc(RpcRequest {
                id: 42,
                method: "propagation_fetch".to_string(),
                params: Some(serde_json::json!({ "transient_id": transient_id })),
            })
            .expect("fetch propagation payload")
            .result
            .expect("fetch result");
        assert_eq!(fetched["payload_hex"].as_str(), Some(hex::encode(lxm_data).as_str()));
    }

    #[tokio::test]
    async fn propagation_envelope_does_not_decode_as_normal_lxmf_delivery() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 5,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                })),
            })
            .expect("enable propagation");
        let transient = stamped_propagation_payload(&[0x42_u8; 113], 1);
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![transient])).expect("propagation envelope");
        let destination = [0x22_u8; 16];

        assert!(inbound_delivery::decode_inbound_payload(
            destination,
            &envelope,
            lxmf::inbound_decode::InboundPayloadMode::FullWire,
        )
        .is_none());
        assert!(ingest_propagation_envelope(&daemon, &envelope, None).await.is_ok());
    }

    #[tokio::test]
    async fn local_propagation_payload_is_decrypted_and_accepted() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 39,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                })),
            })
            .expect("enable propagation");
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let transport_identity = to_transport_private_identity(&to_core_private_identity(&delivery_private));
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "propagated title",
            "propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let envelope = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            let (transient, transient_id) = message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient");
            let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
            WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
                .expect("propagation envelope")
        };
        while daemon.take_event().is_some() {}

        let ingested = ingest_propagation_envelope_with_transport(
            &daemon,
            &envelope,
            Some(&delivery_destination),
            Some(&transport),
        )
        .await
        .expect("ingest propagation envelope");
        assert_eq!(ingested, 1);

        let messages = daemon
            .handle_rpc(RpcRequest { id: 40, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["direction"].as_str(), Some("in"));
        assert_eq!(items[0]["destination"].as_str(), Some(hex::encode(destination_hash).as_str()));
        assert_eq!(items[0]["title"].as_str(), Some("propagated title"));
        assert_eq!(items[0]["content"].as_str(), Some("propagated content"));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_checked"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_valid"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_target_cost"], json!(1));
        assert!(items[0]["fields"]["_lxmf"]["propagation_stamp_value"]
            .as_u64()
            .is_some_and(|value| value >= 1));
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_checked"], json!(false));
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_valid"], json!(false));
        assert_eq!(
            items[0]["fields"]["_lxmf"]["signature_status"],
            json!("source_identity_unknown")
        );

        let event = daemon.take_event().expect("propagated inbound event");
        assert_eq!(event.event_type, "inbound");
        assert_eq!(event.payload["message"]["fields"]["_lxmf"]["signature_checked"], json!(false));
        assert_eq!(event.payload["message"]["fields"]["_lxmf"]["signature_valid"], json!(false));
        assert_eq!(
            event.payload["message"]["fields"]["_lxmf"]["signature_status"],
            json!("source_identity_unknown")
        );
    }

    #[tokio::test]
    async fn local_propagation_payload_records_stamp_inside_flexibility_window() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 44,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 3,
                    "stamp_cost_flexibility": 2,
                })),
            })
            .expect("enable propagation");
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "flex propagated title",
            "flex propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let envelope = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            let (transient, _) = message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient");
            let stamped = stamped_propagation_payload_with_value_range(&transient, 1, 3);
            rmp_serde::to_vec(&(1.0_f64, vec![stamped])).expect("propagation envelope")
        };

        let ingested = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
            .await
            .expect("ingest propagation envelope");
        assert_eq!(ingested, 1);

        let messages = daemon
            .handle_rpc(RpcRequest { id: 45, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"].as_str(), Some("flex propagated title"));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_checked"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_valid"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_target_cost"], json!(1));
        assert!(items[0]["fields"]["_lxmf"]["propagation_stamp_value"]
            .as_u64()
            .is_some_and(|value| (1..3).contains(&value)));
    }

    #[tokio::test]
    async fn inbound_peer_propagation_local_delivery_counts_source_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 46,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                    "stamp_cost_flexibility": 0,
                })),
            })
            .expect("enable propagation");
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
        let propagation_peer = hex::encode([0x7F_u8; 16]);
        let relay_peer = hex::encode([0x80_u8; 16]);
        for (id, peer) in [(47, &propagation_peer), (48, &relay_peer)] {
            daemon
                .handle_rpc(RpcRequest {
                    id,
                    method: "peer_sync".to_string(),
                    params: Some(serde_json::json!({ "peer": peer })),
                })
                .expect("seed propagation peer");
        }

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "peer local propagated title",
            "peer local propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let (envelope, transient_id, transient_len) = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            let (transient, transient_id) = message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient");
            let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
            (
                WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
                    .expect("propagation envelope"),
                hex::encode(transient_id),
                transient.len(),
            )
        };

        let ingested = ingest_propagation_envelope_from_peer(
            &daemon,
            &envelope,
            Some(&delivery_destination),
            Some(&propagation_peer),
        )
        .await
        .expect("ingest peer propagation envelope");
        assert_eq!(ingested, 1);

        let peer = peer_row(&daemon, propagation_peer.as_str(), 49);
        assert_eq!(peer["messages"]["incoming"].as_u64(), Some(1));
        assert_eq!(peer["rx_bytes"].as_u64(), Some(transient_len as u64));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    propagation_peer.as_str(),
                    transient_id.as_str()
                )
                .expect("completed propagation mark lookup"),
            "locally delivered peer propagation payloads should still mark the source peer handled"
        );
        let relay = peer_row(&daemon, relay_peer.as_str(), 50);
        assert_eq!(
            relay["messages"]["unhandled_ids"].as_array().expect("relay unhandled ids"),
            &[serde_json::json!(transient_id.as_str())],
            "locally delivered peer propagation payloads should still fan out to relay peers"
        );

        let status = daemon
            .handle_rpc(RpcRequest {
                id: 51,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn duplicate_local_propagation_payload_does_not_update_peer_activity_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 41,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                })),
            })
            .expect("enable propagation");
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
        let source_hex = hex::encode(source_hash);
        daemon.accept_announce(source_hex.clone(), 1).expect("accept source announce");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "duplicate propagated title",
            "duplicate propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let envelope = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            let (transient, transient_id) = message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient");
            let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
            WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
                .expect("propagation envelope")
        };

        let first_ingested =
            ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
                .await
                .expect("first ingest propagation envelope");
        assert_eq!(first_ingested, 1);
        let after_first = peer_row(&daemon, source_hex.as_str(), 42);
        assert_eq!(after_first["rx_bytes"].as_u64(), Some(wire.len() as u64));
        assert_eq!(after_first["messages"]["incoming"].as_u64(), Some(1));

        let second_ingested =
            ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
                .await
                .expect("second ingest propagation envelope");
        assert_eq!(second_ingested, 1);

        let messages = daemon
            .handle_rpc(RpcRequest { id: 43, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        assert_eq!(messages["messages"].as_array().map(Vec::len), Some(1));
        let after_second = peer_row(&daemon, source_hex.as_str(), 44);
        assert_eq!(after_second["rx_bytes"].as_u64(), Some(wire.len() as u64));
        assert_eq!(after_second["messages"]["incoming"].as_u64(), Some(1));
    }
