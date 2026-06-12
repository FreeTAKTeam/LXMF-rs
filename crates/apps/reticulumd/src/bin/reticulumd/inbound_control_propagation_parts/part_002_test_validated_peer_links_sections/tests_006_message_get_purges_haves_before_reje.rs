    #[test]
    fn message_get_purges_haves_before_rejecting_invalid_wants_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let have = [0x5C; 32];
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                hex::encode(have).as_str(),
                &[],
            )
            .expect("store have payload");

        let response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Integer(7.into()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF4)));
        assert!(
            !daemon.has_propagation_payload(hex::encode(have).as_str()),
            "Python purges haves before later malformed wants abort the request"
        );
    }

    #[test]
    fn message_get_zero_transfer_limit_skips_payload_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x66; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(0u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "zero transfer limit should behave as a real zero-byte budget"
        );
    }

    #[test]
    fn message_get_transfer_limited_wanted_payload_marks_peer_completed_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x67; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(&[0x42; 2_000]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(1u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(messages.is_empty());
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    hex::encode(wanted).as_str(),
                )
                .expect("completed propagation mark lookup"),
            "transfer-limited message-get wants should be completed for this peer"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_transfer_limited_retry_does_not_serve_completed_payload() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x6A; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(&[0x42; 2_000]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let limited_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(1u64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(limited_messages)) = limited_response else {
            panic!("expected limited fetched message list");
        };
        assert!(limited_messages.is_empty());

        let retry_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(retry_messages)) = retry_response else {
            panic!("expected retry fetched message list");
        };
        assert!(
            retry_messages.is_empty(),
            "transfer-limited completed wants should not be served on a later retry"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_cumulative_budget_skip_keeps_later_payload_retryable_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let first = [0x68; 32];
        let second = [0x69; 32];
        let mut first_payload = remote_delivery_hash.to_vec();
        first_payload.extend_from_slice(&[0x42; 900]);
        let mut second_payload = remote_delivery_hash.to_vec();
        second_payload.extend_from_slice(&[0x43; 900]);
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                first_payload.as_slice(),
                hex::encode(first).as_str(),
                &[],
            )
            .expect("store first payload");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                second_payload.as_slice(),
                hex::encode(second).as_str(),
                &[],
            )
            .expect("store second payload");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("record propagation peer");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(first.to_vec()),
                    rmpv::Value::Binary(second.to_vec()),
                ]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(1u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages.len(), 1);
        assert!(daemon
            .has_peer_completed_propagation_mark(
                remote_propagation_hash.as_str(),
                hex::encode(first).as_str(),
            )
            .expect("first completed propagation mark lookup"));
        assert!(
            !daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    hex::encode(second).as_str(),
                )
                .expect("second completed propagation mark lookup"),
            "payloads skipped only by the cumulative response budget should remain retryable"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 13, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(first)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([hex::encode(second)]));
    }

    #[test]
    fn message_get_negative_transfer_limit_skips_payload_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x77; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from(-1i64),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "negative transfer limit should behave as an impossible Python budget"
        );
    }

    #[test]
    fn message_get_string_transfer_limit_parses_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x88; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::from("0"),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "string transfer limits should be parsed through Python float semantics"
        );
    }

    #[test]
    fn message_get_binary_string_transfer_limit_parses_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x99; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" propagation lxm");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(wanted.to_vec())]),
                rmpv::Value::Array(Vec::new()),
                rmpv::Value::Binary(b"0".to_vec()),
            ])),
            0xF1,
            0xF4,
        );
        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert!(
            messages.is_empty(),
            "binary string transfer limits should be parsed through Python float semantics"
        );
    }
