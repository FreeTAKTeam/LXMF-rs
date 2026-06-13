    #[test]
    fn message_get_haves_clear_stale_unhandled_peer_mark_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let have = [0x38; 32];
        let have_hex = hex::encode(have);
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" stale haves cleanup propagation");
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("activate peer");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("store have payload");
        daemon
            .record_peer_unhandled_propagation(remote_propagation_hash.as_str(), have_hex.as_str())
            .expect("mark unhandled");
        let purged = daemon.purge_propagation_payloads_for_destination(
            &remote_delivery_hash,
            std::slice::from_ref(&have.to_vec()),
        );
        assert!(purged > 0);
        daemon
            .record_peer_unhandled_propagation(remote_propagation_hash.as_str(), have_hex.as_str())
            .expect("restore stale unhandled mark");

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
        assert!(list_peer_row(&daemon, remote_propagation_hash.as_str())["messages"]
            ["unhandled_ids"]
            .as_array()
            .expect("unhandled ids")
            .is_empty());
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    have_hex.as_str(),
                )
                .expect("completed propagation mark"),
            "declared haves should survive as completed peer accounting"
        );
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                have_payload.as_slice(),
                have_hex.as_str(),
                &[],
            )
            .expect("reingest stale have payload");
        assert!(
            list_peer_row(&daemon, remote_propagation_hash.as_str())["messages"]["unhandled_ids"]
                .as_array()
                .expect("reingested unhandled ids")
                .is_empty(),
            "reintroduced payload must not be queued back to the declaring peer"
        );
    }

    #[test]
    fn message_get_marks_served_wanted_payloads_transferred_for_peer() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x24; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation accounting lxm");
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
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
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
        assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
        assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_admits_served_peer_for_transfer_accounting_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let wanted = [0x25; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation accounting without prior offer");
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
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(messages)) = fetch_response else {
            panic!("expected fetched message list");
        };
        assert_eq!(messages, vec![rmpv::Value::Binary(wanted_payload)]);
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
            .expect("served peer row");
        assert_eq!(row["peer_type"].as_str(), Some("manual"));
        assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
        assert_eq!(row["messages"]["unhandled"].as_u64(), Some(0));
        assert_eq!(row["messages"]["handled_ids"], json!([hex::encode(wanted)]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn message_get_rejected_peer_does_not_count_or_mark_served_payload() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable static-only propagation");
        let wanted = [0x26; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" rejected peer should not be counted as served");
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
                rmpv::Value::from(10u64),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(fetch_response, ControlResponse::Code(0xF1)));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "rejected message-get peer must not create a peer record"
        );
        let status = daemon
            .handle_rpc(RpcRequest {
                id: 12,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(
            status["propagation"]["client_propagation_messages_served"].as_u64(),
            Some(0),
            "rejected message-get peer must not increment served counters"
        );
    }

    #[test]
    fn message_get_rejected_peer_cannot_list_fetchable_payload_ids() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable static-only propagation");
        let wanted = [0x27; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" rejected peer should not list payload ids");
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                wanted_payload.as_slice(),
                hex::encode(wanted).as_str(),
                &[],
            )
            .expect("store wanted payload");

        let list_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil])),
            0xF1,
            0xF4,
        );

        assert!(matches!(list_response, ControlResponse::Code(0xF1)));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "rejected message-get list must not create a peer record"
        );
    }

    #[test]
    fn message_get_rejected_peer_cannot_purge_haves() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "from_static_only": true,
                    "static_peers": ["not-this-peer"],
                    "peering_cost": 1,
                })),
            })
            .expect("enable static-only propagation");
        let have = [0x28; 32];
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" rejected peer should not purge haves");
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
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(have.to_vec())]),
            ])),
            0xF1,
            0xF4,
        );

        assert!(matches!(response, ControlResponse::Code(0xF1)));
        assert!(
            daemon.has_propagation_payload(hex::encode(have).as_str()),
            "rejected message-get haves must not purge queued payload"
        );
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(
            peers["peers"]
                .as_array()
                .expect("peer rows")
                .iter()
                .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())),
            "rejected message-get haves must not create a peer record"
        );
    }

    #[test]
    fn message_get_ignores_malformed_transient_ids_inside_lists_like_python() {
        let daemon = RpcDaemon::test_instance();
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_delivery_hash = delivery_destination_hash_for_identity(&remote_identity);
        let wanted = [0x5A; 32];
        let have = [0x5B; 32];
        let mut wanted_payload = remote_delivery_hash.to_vec();
        wanted_payload.extend_from_slice(b" wanted propagation lxm");
        let mut have_payload = remote_delivery_hash.to_vec();
        have_payload.extend_from_slice(b" already have propagation lxm");
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

        let fetch_response = handle_message_get_request(
            &daemon,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(vec![0x01; 31]),
                    rmpv::Value::Integer(7.into()),
                    rmpv::Value::Binary(wanted.to_vec()),
                ]),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(vec![0x02; 33]),
                    rmpv::Value::String("not-a-transient-id".into()),
                    rmpv::Value::Binary(have.to_vec()),
                ]),
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
    }
