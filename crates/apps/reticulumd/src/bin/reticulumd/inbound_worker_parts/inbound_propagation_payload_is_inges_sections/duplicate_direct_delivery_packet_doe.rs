    #[tokio::test]
    async fn duplicate_direct_delivery_packet_does_not_update_peer_activity_like_python() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(delivery_destination.desc.address_hash.as_slice());
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
        let source_hex = hex::encode(source_hash);
        daemon.accept_announce(source_hex.clone(), 1).expect("accept source announce");
        let delivery_core_private = to_core_private_identity(&delivery_private);
        let transport_identity = to_transport_private_identity(&delivery_core_private);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "duplicate direct title",
            "duplicate direct content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");

        delivery_events::accept_delivery_packet(
            &daemon,
            &transport,
            hex::encode(destination_hash).as_str(),
            destination_hash,
            &wire,
            ReceivedPayloadMode::FullWire,
        )
        .await;
        let after_first = peer_row(&daemon, source_hex.as_str(), 45);
        assert_eq!(after_first["rx_bytes"].as_u64(), Some(wire.len() as u64));
        assert_eq!(after_first["messages"]["incoming"].as_u64(), Some(1));

        delivery_events::accept_delivery_packet(
            &daemon,
            &transport,
            hex::encode(destination_hash).as_str(),
            destination_hash,
            &wire,
            ReceivedPayloadMode::FullWire,
        )
        .await;

        let messages = daemon
            .handle_rpc(RpcRequest { id: 46, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["fields"]["_lxmf"]["method"], json!(2));
        assert_eq!(items[0]["fields"]["_lxmf"]["transport_encrypted"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["transport_encryption"], json!("Curve25519"));
        let after_second = peer_row(&daemon, source_hex.as_str(), 47);
        assert_eq!(after_second["rx_bytes"].as_u64(), Some(wire.len() as u64));
        assert_eq!(after_second["messages"]["incoming"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn local_propagation_payload_from_ignored_source_is_not_stored_like_python() {
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
        daemon
            .handle_rpc(RpcRequest {
                id: 42,
                method: "set_delivery_policy".to_string(),
                params: Some(serde_json::json!({
                    "ignored_destinations": [hex::encode(source_hash)],
                })),
            })
            .expect("set delivery policy");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "ignored propagated title",
            "ignored propagated content",
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

        let ingested = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
            .await
            .expect("ingest propagation envelope");
        assert_eq!(ingested, 1);

        let messages = daemon
            .handle_rpc(RpcRequest { id: 43, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert!(items.is_empty());
    }

    fn peer_row(daemon: &RpcDaemon, peer: &str, id: u64) -> serde_json::Value {
        let peers = daemon
            .handle_rpc(RpcRequest { id, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(peer))
            .cloned()
            .expect("peer row")
    }

    fn stamped_propagation_payload(lxm_data: &[u8], target_cost: u32) -> Vec<u8> {
        stamped_propagation_payload_with_value_range(lxm_data, target_cost, u32::MAX)
    }

    fn stamped_propagation_payload_with_value_range(
        lxm_data: &[u8],
        min_value: u32,
        max_value: u32,
    ) -> Vec<u8> {
        const PROPAGATION_STAMP_SIZE: usize = 32;
        const PROPAGATION_STAMP_ROUNDS: usize = 1000;

        let transient_id = Sha256::digest(lxm_data);
        let mut workblock = Vec::with_capacity(PROPAGATION_STAMP_ROUNDS * 256);
        for round in 0..PROPAGATION_STAMP_ROUNDS {
            let mut salt_data = Vec::with_capacity(transient_id.len() + 8);
            salt_data.extend_from_slice(transient_id.as_slice());
            let packed =
                rmp_serde::to_vec(&(round as u32)).expect("msgpack encode propagation stamp round");
            salt_data.extend_from_slice(&packed);
            let salt_hash = Sha256::digest(&salt_data);
            let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), transient_id.as_slice());
            let mut okm = [0u8; 256];
            hk.expand(&[], &mut okm).expect("hkdf expand propagation stamp workblock");
            workblock.extend_from_slice(&okm);
        }

        let mut stamp = vec![0u8; PROPAGATION_STAMP_SIZE];
        let mut nonce = 0u64;
        loop {
            stamp[..8].copy_from_slice(&nonce.to_le_bytes());
            let mut material = Vec::with_capacity(workblock.len() + stamp.len());
            material.extend_from_slice(&workblock);
            material.extend_from_slice(&stamp);
            let hash = Sha256::digest(&material);
            let mut value = 0u32;
            for byte in hash {
                if byte == 0 {
                    value += 8;
                } else {
                    value += byte.leading_zeros();
                    break;
                }
            }
            if value >= min_value && value < max_value {
                break;
            }
            nonce = nonce.wrapping_add(1);
        }

        let mut transient = lxm_data.to_vec();
        transient.extend_from_slice(&stamp);
        transient
    }
