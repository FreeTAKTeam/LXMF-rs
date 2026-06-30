#[cfg(test)]
mod tests {
    use super::*;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use rns_transport::destination::DestinationName;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{to_core_identity, to_core_private_identity};
    use rns_transport::ratchets::encrypt_for_public_key;
    use tokio::sync::Mutex as TokioMutex;

    #[test]
    fn propagation_download_summary_reports_transferred_bytes() {
        let payloads = vec![b"downloaded".to_vec(), b"payload-two".to_vec()];
        let transient_ids = vec![vec![0x33; 32], vec![0x44; 32]];

        let summary = propagation_download_summary_json(5, &payloads, &transient_ids, 1, 1, 2);

        assert_eq!(summary["available_count"].as_u64(), Some(5));
        assert_eq!(summary["downloaded_count"].as_u64(), Some(1));
        assert_eq!(summary["duplicate_count"].as_u64(), Some(1));
        assert_eq!(summary["rejected_count"].as_u64(), Some(2));
        assert_eq!(summary["available"].as_u64(), Some(5));
        assert_eq!(summary["downloaded"].as_u64(), Some(1));
        assert_eq!(summary["duplicates"].as_u64(), Some(1));
        assert_eq!(summary["rejected"].as_u64(), Some(2));
        assert_eq!(
            summary["transferred_bytes"].as_u64(),
            Some(payloads.iter().map(Vec::len).sum::<usize>() as u64)
        );
        let messages = summary["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 2);
        let expected_payload_hex = hex::encode(&payloads[0]);
        let expected_transient_id = hex::encode(&transient_ids[0]);
        assert_eq!(messages[0]["payload_hex"].as_str(), Some(expected_payload_hex.as_str()));
        assert_eq!(messages[0]["transient_id"].as_str(), Some(expected_transient_id.as_str()));
    }

    #[test]
    fn propagation_download_summary_preserves_advertised_transient_id() {
        let payloads = vec![vec![0x42; 272]];
        let advertised_id = vec![0x19; 32];

        let summary =
            propagation_download_summary_json(1, &payloads, std::slice::from_ref(&advertised_id), 1, 0, 0);
        let messages = summary["messages"].as_array().expect("messages");

        assert_eq!(messages[0]["transient_id"].as_str(), Some(hex::encode(advertised_id).as_str()));
        assert_eq!(messages[0]["payload_hex"].as_str(), Some(hex::encode(&payloads[0]).as_str()));
    }

    #[test]
    fn classify_remote_transient_ids_reports_known_entries_as_haves() {
        let known = [0x11; 32];
        let unknown = [0x22; 32];

        let (wants, haves) = classify_remote_transient_ids_with(
            vec![known.to_vec(), unknown.to_vec()],
            |transient_id| Ok(transient_id == known.as_slice()),
        )
        .expect("classify remote ids");

        assert_eq!(wants, vec![unknown.to_vec()]);
        assert_eq!(haves, vec![known.to_vec()]);
    }

    #[test]
    fn propagation_download_get_payload_sends_mixed_wants_and_haves() {
        let wanted = vec![vec![0x11; 32]];
        let haves = vec![vec![0x22; 32]];

        let data = decode_link_request_payload(
            propagation_download_get_payload(Some(wanted.as_slice()), haves.as_slice(), Some(42.0))
                .expect("build get payload")
                .as_slice(),
        );

        let rmpv::Value::Array(entries) = data else {
            panic!("request data should be an array");
        };
        assert_eq!(
            entries.first(),
            Some(&rmpv::Value::Array(vec![rmpv::Value::Binary(wanted[0].clone())]))
        );
        assert_eq!(
            entries.get(1),
            Some(&rmpv::Value::Array(vec![rmpv::Value::Binary(haves[0].clone())]))
        );
        assert_eq!(entries.get(2).and_then(rmpv::Value::as_f64), Some(42.0));
    }

    #[test]
    fn propagation_download_get_payload_sends_purge_only_when_no_wants() {
        let haves = vec![vec![0x33; 32]];

        let data = decode_link_request_payload(
            propagation_download_get_payload(None, haves.as_slice(), None)
                .expect("build purge payload")
                .as_slice(),
        );

        let rmpv::Value::Array(entries) = data else {
            panic!("request data should be an array");
        };
        assert!(entries.first().is_some_and(rmpv::Value::is_nil));
        assert_eq!(
            entries.get(1),
            Some(&rmpv::Value::Array(vec![rmpv::Value::Binary(haves[0].clone())]))
        );
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn propagation_download_ack_rejects_remote_error_code() {
        let err = propagation_download_ack_response_result(&rmpv::Value::from(0xF6_u8))
            .expect_err("throttled ack response should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        assert!(err.to_string().contains("throttled"));
    }

    #[test]
    fn propagation_download_haves_only_summary_requires_ack_success() {
        let err = propagation_download_haves_only_summary(2, &rmpv::Value::from(0xF4_u64))
            .expect_err("remote cleanup rejection must fail purge-only download");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("rejected"));

        let summary = propagation_download_haves_only_summary(2, &rmpv::Value::Boolean(true))
            .expect("successful ack returns summary");

        assert_eq!(summary["available_count"].as_u64(), Some(2));
        assert_eq!(summary["downloaded_count"].as_u64(), Some(0));
        assert_eq!(summary["duplicate_count"].as_u64(), Some(0));
        assert_eq!(summary["rejected_count"].as_u64(), Some(0));
        assert_eq!(summary["transferred_bytes"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn policy_rejected_downloaded_payload_is_not_reported_as_duplicate_have() {
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
                id: 70,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "ignored_destinations": [hex::encode(source_hash)],
                })),
            })
            .expect("set delivery policy");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "ignored remote title",
            "ignored remote content",
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

        let outcome = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            transient_payload.as_slice(),
            None,
        )
        .await
        .expect("accept downloaded payload");

        assert_eq!(
            outcome,
            DownloadAcceptOutcome::Rejected,
            "policy-rejected downloads are not local haves and must not be acked"
        );

        assert_downloaded_drop_event(
            &daemon,
            "delivery_policy_rejected",
            destination_hash,
            wire.len(),
            |event| {
                assert!(event.payload["source_hash"].as_str().is_some_and(
                    |value| value.starts_with("sha256:") && value != hex::encode(source_hash)
                ));
                assert!(event.payload.get("detail").is_none());
            },
        );
        assert!(daemon.take_event().is_none(), "rejected downloaded payload should emit one event");
    }

    #[tokio::test]
    async fn malformed_downloaded_payload_emits_bounded_decode_drop_event() {
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
            let plaintext = b"not-a-valid-downloaded-lxmf-payload";
            (
                destination_hash,
                encrypted_downloaded_transient(&destination, plaintext),
                destination_hash.len() + plaintext.len(),
            )
        };

        let err = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            transient_payload.as_slice(),
            None,
        )
        .await
        .expect_err("malformed downloaded payload should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_downloaded_drop_event(&daemon, "decode_failed", destination_hash, wire_len, |event| {
            assert!(
                event.payload["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains("full_wire")),
                "downloaded decode drop should include bounded diagnostics: {:?}",
                event.payload
            );
        });
        assert!(daemon.take_event().is_none(), "malformed downloaded payload should emit one event");
    }

    #[tokio::test]
    async fn downloaded_predecode_failures_emit_drop_events() {
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
        let err = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            too_short.as_slice(),
            None,
        )
        .await
        .expect_err("too-short downloaded payload should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_downloaded_drop_event(&daemon, "payload_too_short", destination_hash, too_short.len(), |event| {
            assert_eq!(event.payload["detail"], json!("propagated LXMF payload too short"));
        });

        let mut mismatch = vec![0x22_u8; 16 + 33];
        mismatch[..16].copy_from_slice(&[0x99_u8; 16]);
        let err = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            mismatch.as_slice(),
            None,
        )
        .await
        .expect_err("mismatched downloaded payload should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_downloaded_drop_event(
            &daemon,
            "destination_mismatch",
            destination_hash,
            mismatch.len(),
            |event| {
                assert_eq!(
                    event.payload["detail"],
                    json!("propagated LXMF payload is not addressed to local delivery destination")
                );
            },
        );

        let mut undecryptable = vec![0x33_u8; 16 + 33];
        undecryptable[..16].copy_from_slice(&destination_hash);
        let err = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            undecryptable.as_slice(),
            None,
        )
        .await
        .expect_err("undecryptable downloaded payload should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_downloaded_drop_event(
            &daemon,
            "decrypt_failed",
            destination_hash,
            undecryptable.len(),
            |event| {
                assert_eq!(
                    event.payload["detail"],
                    json!("failed to decrypt downloaded propagated LXMF payload")
                );
            },
        );
        assert!(daemon.take_event().is_none(), "predecode downloaded failures should emit one event each");
    }

    #[tokio::test]
    async fn unstamped_downloaded_payload_emits_stamp_policy_drop_event() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 71,
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
            "unstamped remote title",
            "unstamped remote content",
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

        let err = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            transient_payload.as_slice(),
            None,
        )
        .await
        .expect_err("unstamped downloaded payload should fail stamp policy");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_downloaded_drop_event(
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
        assert!(daemon.take_event().is_none(), "unstamped downloaded payload should emit one event");
    }

    #[test]
    fn propagation_download_ack_rejects_remote_rejection_code() {
        let err = propagation_download_ack_response_result(&rmpv::Value::from(0xF4_u64))
            .expect_err("remote ack rejection must fail the download");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("rejected"));
    }

    fn encrypted_downloaded_transient(
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
        .expect("encrypt downloaded transient");
        let mut transient = Vec::with_capacity(destination.desc.address_hash.as_slice().len() + encrypted.len());
        transient.extend_from_slice(destination.desc.address_hash.as_slice());
        transient.extend_from_slice(encrypted.as_slice());
        transient
    }

    fn assert_downloaded_drop_event(
        daemon: &RpcDaemon,
        reason: &str,
        destination_hash: [u8; 16],
        bytes_len: usize,
        extra: impl FnOnce(&rns_rpc::RpcEvent),
    ) {
        let event = daemon.take_event().expect("downloaded drop event");
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
}
