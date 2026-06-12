    #[test]
    fn message_get_lists_fetches_and_purges_remote_delivery_payloads() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let other_delivery_hash = [0x44; 16];
        let wanted = [0x22; 32];
        let have = [0x33; 32];
        let ignored = [0x55; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation lxm");
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation lxm");
        let mut ignored_payload = other_delivery_hash.to_vec();
        ignored_payload.extend_from_slice(b" other recipient");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                hex::encode(have).as_str(),
                &[],
            )
            .expect("store have payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                ignored_payload.as_slice(),
                hex::encode(ignored).as_str(),
                &[],
            )
            .expect("store ignored payload");

        let list_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(available)) = list_response else {
            panic!("expected available transient id list");
        };
        assert_eq!(
            available,
            vec![rmpv::Value::Binary(wanted.to_vec()), rmpv::Value::Binary(have.to_vec())]
        );

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
        assert!(!daemon.has_propagation_payload(hex::encode(have).as_str()));
        assert!(daemon.has_propagation_payload(hex::encode(ignored).as_str()));
    }

    #[test]
    fn message_get_haves_mark_requesting_peer_received_across_purge_and_reingest() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let have = [0x36; 32];
        let have_hex = hex::encode(have);
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation accounting lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("store have payload");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Bool(true)));
        assert!(!daemon.has_propagation_payload(have_hex.as_str()));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("completed propagation mark lookup"),
            "message-get haves should be remembered as peer-received after local purge"
        );

        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("reingest have payload");
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("completed propagation mark after reingest"),
            "reingesting a purged payload must not forget that the peer already has it"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 15, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(
            row["messages"]["unhandled_ids"],
            json!([]),
            "reingested haves should not be queued back to the declaring peer"
        );
    }

    #[test]
    fn message_get_unknown_haves_do_not_complete_future_peer_work() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("activate requesting peer");
        let future_have = [0x39; 32];
        let future_have_hex = hex::encode(future_have);

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(future_have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Bool(true)));
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    future_have_hex.as_str(),
                )
                .expect("completed propagation mark lookup"),
            "unknown haves must not pre-complete future propagation work"
        );

        let mut future_payload = remote_delivery_hash.to_vec();
        future_payload.extend_from_slice(b" future haves should still queue");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                future_payload.as_slice(),
                future_have_hex.as_str(),
                &[],
            )
            .expect("ingest future payload");
        let row = list_peer_row(&daemon, remote_propagation_hash.as_str());
        assert_eq!(
            row["messages"]["unhandled_ids"],
            json!([future_have_hex.as_str()]),
            "future payloads must still be queued to peers that only declared unknown haves"
        );
    }

    #[test]
    fn message_get_haves_retains_payload_when_retain_synced_on_node_enabled() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 16,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "retain_synced_on_node": true,
                })),
            })
            .expect("enable propagation retention");
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let other_peer = "ba".repeat(16);
        let have = [0x38; 32];
        let have_hex = hex::encode(have);
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" retained synced propagation payload");
        daemon.record_propagation_offer_peer(other_peer.as_str()).expect("activate other peer");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("store retained have payload");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Bool(true)));
        assert!(
            daemon.has_propagation_payload(have_hex.as_str()),
            "retain-synced propagation nodes should keep payloads after haves"
        );
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("requesting peer completed mark"),
            "retained haves should still complete the requesting peer"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 15, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let other_row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(other_peer.as_str()))
            .expect("other peer row");
        assert_eq!(
            other_row["messages"]["unhandled_ids"],
            json!([have_hex.as_str()]),
            "retained payload should remain queued for peers that have not completed it"
        );

        let list_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(available)) = list_response else {
            panic!("expected retained available transient id list");
        };
        assert!(
            available.is_empty(),
            "retained haves should not be listed back to the peer that completed them"
        );
    }

    #[test]
    fn message_get_haves_preserve_other_peer_completed_marks_across_purge_and_reingest() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let other_peer = "ab".repeat(16);
        let have = [0x37; 32];
        let have_hex = hex::encode(have);
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" haves should preserve other peer accounting");
        daemon.record_propagation_offer_peer(other_peer.as_str()).expect("activate other peer");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("store have payload");
        daemon
            .record_peer_transferred_propagation(other_peer.as_str(), have_hex.as_str())
            .expect("mark other peer transferred");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Bool(true)));
        assert!(!daemon.has_propagation_payload(have_hex.as_str()));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(other_peer.as_str(), have_hex.as_str())
                .expect("other peer completed mark"),
            "purging one peer's haves must not erase another peer's completed mark"
        );
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("requesting peer completed mark"),
            "requesting peer should still be marked completed after purge"
        );

        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("reingest have payload");
        let peers = daemon
            .handle_rpc(RpcRequest { id: 16, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(other_peer.as_str()))
            .expect("other peer row");
        assert_eq!(
            row["messages"]["unhandled_ids"],
            json!([]),
            "reingested payload must not be requeued to a peer that already completed it"
        );
    }
