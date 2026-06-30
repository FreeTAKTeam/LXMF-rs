    use super::*;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use reticulum_daemon::lxmf_stamps::generate_propagation_stamp;
    use rns_transport::destination::DestinationName;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{
        to_core_identity, to_core_private_identity, to_transport_private_identity,
    };
    use rns_transport::ratchets::encrypt_for_public_key;
    use rns_transport::transport::{Transport, TransportConfig};
    use tokio::sync::Mutex as TokioMutex;

    #[tokio::test]
    async fn policy_rejected_fetched_payload_is_reported_separately_from_duplicate() {
        let daemon = RpcDaemon::test_instance();
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
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
        daemon
            .handle_rpc(RpcRequest {
                id: 80,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "ignored_destinations": [hex::encode(source_hash)],
                })),
            })
            .expect("set delivery policy");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "ignored fetch title",
            "ignored fetch content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let transient_payload = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient")
                .0
        };

        let outcome = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            &transient_payload,
            None,
        )
        .await
        .expect("accept fetched payload");

        assert_eq!(
            outcome,
            LocalPropagationImportOutcome::Rejected,
            "policy-rejected fetched payloads should not be counted as duplicates"
        );

        let event = daemon.take_event().expect("fetched drop event");
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
        assert!(
            event.payload["source_hash"].as_str().is_some_and(
                |value| value.starts_with("sha256:") && value != hex::encode(source_hash)
            )
        );
        assert_eq!(event.payload["payload_mode"], json!("full_wire"));
        assert_eq!(event.payload["bytes_len"], json!(wire.len()));
        assert!(daemon.take_event().is_none(), "rejected fetched payload should emit one event");
    }

    #[tokio::test]
    async fn malformed_fetched_payload_emits_bounded_decode_drop_event() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private,
            DestinationName::new("lxmf", "delivery"),
        )));
        let (destination_hash, transient_payload, wire_len) = {
            let destination = delivery_destination.lock().await;
            let mut destination_hash = [0u8; 16];
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
            let plaintext = b"not-a-valid-fetched-lxmf-payload";
            (
                destination_hash,
                encrypted_fetched_transient(&destination, plaintext),
                destination_hash.len() + plaintext.len(),
            )
        };

        let err = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            transient_payload.as_slice(),
            None,
        )
        .await
        .expect_err("malformed fetched payload should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_fetched_drop_event(&daemon, "decode_failed", destination_hash, wire_len, |event| {
            assert!(
                event.payload["detail"].as_str().is_some_and(|detail| detail.contains("full_wire")),
                "fetched decode drop should include bounded diagnostics: {:?}",
                event.payload
            );
        });
        assert!(daemon.take_event().is_none(), "malformed fetched payload should emit one event");
    }

    #[tokio::test]
    async fn fetched_predecode_failures_emit_drop_events() {
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

        let too_short = vec![0x11_u8; 8];
        let err = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination.clone(),
            too_short.as_slice(),
            None,
        )
        .await
        .expect_err("too-short fetched payload should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_fetched_drop_event(&daemon, "payload_too_short", destination_hash, too_short.len(), |event| {
            assert_eq!(event.payload["detail"], json!("propagated LXMF payload too short"));
        });

        let mut mismatch = vec![0x22_u8; 16 + 33];
        mismatch[..16].copy_from_slice(&[0x99_u8; 16]);
        let err = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination.clone(),
            mismatch.as_slice(),
            None,
        )
        .await
        .expect_err("mismatched fetched payload should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_fetched_drop_event(
            &daemon,
            "destination_mismatch",
            destination_hash,
            mismatch.len(),
            |event| {
                assert_eq!(
                    event.payload["detail"],
                    json!("propagated LXMF payload is not addressed to the local delivery destination")
                );
            },
        );

        let mut undecryptable = vec![0x33_u8; 16 + 33];
        undecryptable[..16].copy_from_slice(&destination_hash);
        let err = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            undecryptable.as_slice(),
            None,
        )
        .await
        .expect_err("undecryptable fetched payload should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_fetched_drop_event(
            &daemon,
            "decrypt_failed",
            destination_hash,
            undecryptable.len(),
            |event| {
                assert_eq!(
                    event.payload["detail"],
                    json!("failed to decrypt propagated LXMF payload for local delivery")
                );
            },
        );
        assert!(daemon.take_event().is_none(), "predecode fetched failures should emit one event each");
    }

    #[tokio::test]
    async fn unstamped_fetched_payload_emits_stamp_policy_drop_event() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 81,
                method: "stamp_policy_set".to_string(),
                params: Some(json!({"target_cost": 4, "flexibility": 0})),
            })
            .expect("set stamp policy");
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
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "unstamped fetch title",
            "unstamped fetch content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let transient_payload = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient")
                .0
        };

        let err = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            transient_payload.as_slice(),
            None,
        )
        .await
        .expect_err("unstamped fetched payload should fail stamp policy");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_fetched_drop_event(
            &daemon,
            "stamp_policy_rejected",
            destination_hash,
            wire.len(),
            |event| {
                let raw_source = hex::encode(source_hash);
                assert!(
                    event.payload["detail"].as_str().is_some_and(
                        |detail| detail == "invalid LXMF stamp" && !detail.contains(raw_source.as_str())
                    ),
                    "stamp drop should include policy detail: {:?}",
                    event.payload
                );
                assert!(event.payload["source_hash"]
                    .as_str()
                    .is_some_and(|value| value.starts_with("sha256:") && value != raw_source));
            },
        );
        assert!(daemon.take_event().is_none(), "unstamped fetched payload should emit one event");
    }

    #[tokio::test]
    async fn duplicate_fetched_payload_is_reported_separately_from_rejection() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let transport_identity =
            to_transport_private_identity(&to_core_private_identity(&delivery_private));
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
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "duplicate fetch title",
            "duplicate fetch content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let transient_payload = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient")
                .0
        };

        let first = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination.clone(),
            &transient_payload,
            Some(&transport),
        )
        .await
        .expect("first fetch accept");
        let second = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            &transient_payload,
            None,
        )
        .await
        .expect("second fetch accept");

        assert_eq!(first, LocalPropagationImportOutcome::Imported);
        assert_eq!(second, LocalPropagationImportOutcome::Duplicate);
        let messages = daemon
            .handle_rpc(RpcRequest { id: 82, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_checked"], json!(false));
        assert_eq!(items[0]["fields"]["_lxmf"]["signature_valid"], json!(false));
        assert_eq!(
            items[0]["fields"]["_lxmf"]["signature_status"],
            json!("source_identity_unknown")
        );
        let event = daemon.take_event().expect("fetched inbound event");
        assert_eq!(event.event_type, "inbound");
        assert_eq!(event.payload["message"]["fields"]["_lxmf"]["signature_checked"], json!(false));
        assert_eq!(event.payload["message"]["fields"]["_lxmf"]["signature_valid"], json!(false));
        assert_eq!(
            event.payload["message"]["fields"]["_lxmf"]["signature_status"],
            json!("source_identity_unknown")
        );
    }

    #[test]
    fn ack_transient_id_uses_lxm_data_for_stamped_payloads() {
        let lxm_data = vec![0x42; 160];
        let transient_id = Sha256::digest(&lxm_data);
        let stamp = generate_propagation_stamp(
            transient_id.as_slice().try_into().expect("transient id width"),
            1,
        )
        .expect("propagation stamp");
        let mut transient_payload = lxm_data.clone();
        transient_payload.extend_from_slice(stamp.as_slice());

        let ack_id = propagation_payload_ack_transient_id(transient_payload.as_slice());

        assert_eq!(ack_id, transient_id.to_vec());
        assert_ne!(ack_id, Sha256::digest(transient_payload).to_vec());
    }

    #[test]
    fn ack_transient_id_keeps_unstamped_payload_hash() {
        let transient_payload = b"ack-unstamped-lxm-data".to_vec();

        let ack_id = propagation_payload_ack_transient_id(transient_payload.as_slice());

        assert_eq!(ack_id, Sha256::digest(transient_payload).to_vec());
    }

    fn encrypted_fetched_transient(
        destination: &SingleInputDestination,
        plaintext: &[u8],
    ) -> Vec<u8> {
        let identity = destination.identity.as_identity();
        let encrypted = encrypt_for_public_key(
            &identity.public_key,
            identity.address_hash.as_slice(),
            plaintext,
            OsRng,
        )
        .expect("encrypt fetched transient");
        let mut transient =
            Vec::with_capacity(destination.desc.address_hash.as_slice().len() + encrypted.len());
        transient.extend_from_slice(destination.desc.address_hash.as_slice());
        transient.extend_from_slice(encrypted.as_slice());
        transient
    }

    fn assert_fetched_drop_event(
        daemon: &RpcDaemon,
        reason: &str,
        destination_hash: [u8; 16],
        bytes_len: usize,
        extra: impl FnOnce(&rns_rpc::RpcEvent),
    ) {
        let event = daemon.take_event().expect("fetched drop event");
        assert_eq!(event.event_type, "inbound_dropped");
        assert_eq!(event.payload["reason"], json!(reason));
        assert_eq!(event.payload["delivery_kind"], json!("propagation"));
        let raw_destination = hex::encode(destination_hash);
        assert!(event.payload["raw_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert!(event.payload["resolved_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination));
        assert_eq!(event.payload["payload_mode"], json!("full_wire"));
        assert_eq!(event.payload["bytes_len"], json!(bytes_len));
        extra(&event);
    }
