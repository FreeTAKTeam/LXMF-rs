#[cfg(test)]
mod tests {
    use super::*;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use rns_transport::destination::DestinationName;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{to_core_identity, to_core_private_identity};
    use tokio::sync::Mutex as TokioMutex;

    #[test]
    fn propagation_download_summary_reports_transferred_bytes() {
        let payloads = vec![b"downloaded".to_vec(), b"payload-two".to_vec()];

        let summary = propagation_download_summary_json(5, &payloads, 1, 1, 2);

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
        )
        .await
        .expect("accept downloaded payload");

        assert_eq!(
            outcome,
            DownloadAcceptOutcome::Rejected,
            "policy-rejected downloads are not local haves and must not be acked"
        );
    }

    #[test]
    fn propagation_download_ack_rejects_remote_rejection_code() {
        let err = propagation_download_ack_response_result(&rmpv::Value::from(0xF4_u64))
            .expect_err("remote ack rejection must fail the download");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("rejected"));
    }
}
