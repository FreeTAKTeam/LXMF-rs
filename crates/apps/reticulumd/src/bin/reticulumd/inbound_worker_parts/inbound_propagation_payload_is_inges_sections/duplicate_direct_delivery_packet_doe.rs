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
    async fn malformed_direct_delivery_emits_drop_event_without_peer_or_message_side_effects() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(delivery_destination.desc.address_hash.as_slice());
        let delivery_core_private = to_core_private_identity(&delivery_private);
        let transport_identity = to_transport_private_identity(&delivery_core_private);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let malformed_payload = b"not-a-valid-lxmf-wire-payload";

        delivery_events::accept_delivery_packet(
            &daemon,
            &transport,
            hex::encode(destination_hash).as_str(),
            destination_hash,
            malformed_payload,
            ReceivedPayloadMode::FullWire,
        )
        .await;

        let event = daemon.take_event().expect("drop event");
        assert_eq!(event.event_type, "inbound_dropped");
        assert_eq!(event.payload["reason"], json!("decode_failed"));
        assert_eq!(event.payload["delivery_kind"], json!("packet"));
        let raw_destination = hex::encode(destination_hash);
        assert!(event.payload["raw_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert!(event.payload["resolved_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert_eq!(event.payload["payload_mode"], json!("full_wire"));
        assert_eq!(event.payload["bytes_len"], json!(malformed_payload.len()));
        assert!(
            event.payload["detail"].as_str().is_some_and(|detail| detail.contains("full_wire")),
            "drop event should include bounded decode diagnostics: {:?}",
            event.payload
        );
        assert!(daemon.take_event().is_none(), "malformed direct delivery should emit one event");

        let messages = daemon
            .handle_rpc(RpcRequest { id: 48, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        assert_eq!(messages["messages"].as_array().expect("message items").len(), 0);

        let peers = daemon
            .handle_rpc(RpcRequest { id: 49, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert_eq!(peers["peers"].as_array().expect("peer rows").len(), 0);
    }

    #[tokio::test]
    async fn malformed_direct_delivery_resource_emits_drop_event_without_side_effects() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(delivery_destination.desc.address_hash.as_slice());
        let delivery_core_private = to_core_private_identity(&delivery_private);
        let transport_identity = to_transport_private_identity(&delivery_core_private);
        let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
        let malformed_payload = b"not-a-valid-lxmf-resource-payload";

        delivery_events::accept_delivery_resource(
            &daemon,
            &transport,
            destination_hash,
            malformed_payload,
        )
        .await;

        let event = daemon.take_event().expect("drop event");
        assert_eq!(event.event_type, "inbound_dropped");
        assert_eq!(event.payload["reason"], json!("decode_failed"));
        assert_eq!(event.payload["delivery_kind"], json!("resource"));
        let raw_destination = hex::encode(destination_hash);
        assert!(event.payload["raw_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert!(event.payload["resolved_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert_eq!(event.payload["payload_mode"], json!("full_wire"));
        assert_eq!(event.payload["bytes_len"], json!(malformed_payload.len()));
        assert!(
            event.payload["detail"].as_str().is_some_and(|detail| detail.contains("full_wire")),
            "drop event should include bounded decode diagnostics: {:?}",
            event.payload
        );
        assert!(daemon.take_event().is_none(), "malformed resource should emit one event");

        let messages = daemon
            .handle_rpc(RpcRequest { id: 50, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        assert_eq!(messages["messages"].as_array().expect("message items").len(), 0);

        let peers = daemon
            .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert_eq!(peers["peers"].as_array().expect("peer rows").len(), 0);
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

        let event = daemon.take_event().expect("propagated drop event");
        assert_eq!(event.event_type, "inbound_dropped");
        assert_eq!(event.payload["reason"], json!("delivery_policy_rejected"));
        assert_eq!(event.payload["delivery_kind"], json!("propagation"));
        let raw_destination = hex::encode(destination_hash);
        assert!(event.payload["raw_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert!(event.payload["resolved_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert!(event.payload["source_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != hex::encode(source_hash)));
        assert_eq!(event.payload["payload_mode"], json!("full_wire"));
        assert_eq!(event.payload["bytes_len"], json!(wire.len()));
        assert!(daemon.take_event().is_none(), "ignored propagated payload should emit one event");

        let messages = daemon
            .handle_rpc(RpcRequest { id: 43, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn local_propagation_predecode_drop_is_visible_to_sdk_poll_events() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private,
            DestinationName::new("lxmf", "delivery"),
        )));
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
        let pre_poll = daemon
            .handle_rpc(RpcRequest {
                id: 44,
                method: "sdk_poll_events_v2".to_string(),
                params: Some(json!({ "cursor": null, "max": 20 })),
            })
            .expect("pre poll sdk events")
            .result
            .expect("pre poll result");
        let pre_cursor = pre_poll["next_cursor"].as_str().expect("pre cursor").to_owned();
        let mut transient_payload = destination_hash.to_vec();
        transient_payload.extend_from_slice(&[0xA5_u8; 8]);
        let envelope = rmp_serde::to_vec(&(1.0_f64, vec![transient_payload.clone()]))
            .expect("propagation envelope");

        let ingested = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
            .await
            .expect("ingest predecode propagation envelope");
        assert_eq!(ingested, 1);

        let poll = daemon
            .handle_rpc(RpcRequest {
                id: 45,
                method: "sdk_poll_events_v2".to_string(),
                params: Some(json!({ "cursor": pre_cursor, "max": 20 })),
            })
            .expect("poll sdk events")
            .result
            .expect("poll result");
        let events = poll["events"].as_array().expect("event rows");
        let event = events
            .iter()
            .find(|event| event["event_type"] == json!("inbound_dropped"))
            .expect("sdk propagated drop event");
        assert_eq!(event["payload"]["reason"], json!("payload_too_short"));
        assert_eq!(event["payload"]["delivery_kind"], json!("propagation"));
        assert_eq!(event["payload"]["payload_mode"], json!("full_wire"));
        assert_eq!(event["payload"]["bytes_len"], json!(transient_payload.len()));
        assert!(
            event["payload"]["raw_destination_hash"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:") && value != hex::encode(destination_hash))
        );
        assert!(
            event["payload"]["detail"]
                .as_str()
                .is_some_and(|detail| detail == "propagated LXMF payload too short")
        );

        let messages = daemon
            .handle_rpc(RpcRequest { id: 46, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        assert!(messages["messages"].as_array().expect("message items").is_empty());
        let peers = daemon
            .handle_rpc(RpcRequest { id: 47, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(peers["peers"].as_array().expect("peer rows").is_empty());
    }

    #[tokio::test]
    async fn peer_local_propagation_predecode_drop_does_not_relay_or_mark_completed() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private,
            DestinationName::new("lxmf", "delivery"),
        )));
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
        let source_peer = hex::encode([0x81_u8; 16]);
        let relay_peer = hex::encode([0x82_u8; 16]);
        for (id, peer) in [(48, &source_peer), (49, &relay_peer)] {
            daemon
                .handle_rpc(RpcRequest {
                    id,
                    method: "peer_sync".to_string(),
                    params: Some(json!({ "peer": peer })),
                })
                .expect("seed propagation peer");
        }
        while daemon.take_event().is_some() {}
        let mut transient_payload = destination_hash.to_vec();
        transient_payload.extend_from_slice(&[0xB5_u8; 8]);
        let transient_id = hex::encode(Sha256::digest(&transient_payload));
        let envelope = rmp_serde::to_vec(&(1.0_f64, vec![transient_payload.clone()]))
            .expect("propagation envelope");

        let ingested = ingest_propagation_envelope_from_peer(
            &daemon,
            &envelope,
            Some(&delivery_destination),
            Some(&source_peer),
        )
        .await
        .expect("ingest peer predecode propagation envelope");
        assert_eq!(ingested, 1);

        let event = daemon.take_event().expect("peer predecode drop event");
        assert_eq!(event.event_type, "inbound_dropped");
        assert_eq!(event.payload["reason"], json!("payload_too_short"));
        assert_eq!(event.payload["delivery_kind"], json!("propagation"));
        assert!(daemon.take_event().is_none(), "peer predecode drop should emit one event");
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(source_peer.as_str(), transient_id.as_str())
                .expect("completed propagation mark lookup"),
            "dropped predecode peer payloads must not mark the source peer handled"
        );
        let source = peer_row(&daemon, source_peer.as_str(), 50);
        assert_eq!(source["messages"]["incoming"].as_u64(), Some(0));
        assert_eq!(source["messages"]["unhandled_ids"], json!([]));
        let relay = peer_row(&daemon, relay_peer.as_str(), 51);
        assert_eq!(
            relay["messages"]["unhandled_ids"],
            json!([]),
            "dropped predecode peer payloads must not fan out to relay peers"
        );
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
            let packed = rmp_serde::to_vec(&round).expect("msgpack encode propagation stamp round");
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
