    use super::*;

    use reticulum_daemon::lxmf_stamps::generate_peering_key;

    use std::collections::HashSet;

    use std::sync::{Arc, Mutex};

    fn test_validated_peer_links() -> Arc<Mutex<HashSet<AddressHash>>> {
        Arc::new(Mutex::new(HashSet::new()))
    }

    fn test_link_id() -> AddressHash {
        AddressHash::new([0xA6; 16])
    }

    fn list_peer_row(daemon: &RpcDaemon, peer: &str) -> serde_json::Value {
        daemon
            .handle_rpc(RpcRequest { id: 90, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result")["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(peer))
            .cloned()
            .expect("peer row")
    }

    #[test]
    fn offer_request_returns_only_missing_transient_ids_after_peering_key_validation() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let existing = [0xAA; 32];
        let missing = [0xBB; 32];
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                b"stored propagation payload",
                hex::encode(existing).as_str(),
                &[],
            )
            .expect("store existing payload");

        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let link_id = test_link_id();
        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(existing.to_vec()),
                    rmpv::Value::Binary(missing.to_vec()),
                ]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        let ControlResponse::Rmpv(rmpv::Value::Array(wanted)) = response else {
            panic!("expected partial wanted-id list");
        };
        assert_eq!(wanted, vec![rmpv::Value::Binary(missing.to_vec())]);
        assert!(control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
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
            "wanted offers should validate the link without admitting or queueing the peer"
        );
    }

    #[test]
    fn offer_request_empty_offer_does_not_queue_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let existing_payload = b"queued propagation payload";
        let existing_transient_id = hex::encode(sha2::Sha256::digest(existing_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                existing_payload,
                existing_transient_id.as_str(),
                &[],
            )
            .expect("store existing payload");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let link_id = test_link_id();
        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(Vec::new()),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
        assert!(control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())));
    }

    #[test]
    fn offer_request_all_known_offer_does_not_queue_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let known_payload = b"known propagation offer payload";
        let known_transient_id = hex::encode(sha2::Sha256::digest(known_payload));
        daemon
            .ingest_propagation_payload_bytes_with_aliases(
                known_payload,
                known_transient_id.as_str(),
                &[],
            )
            .expect("store known payload");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let link_id = test_link_id();
        let response = handle_offer_request(
            &daemon,
            &control,
            &link_id,
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(
                    hex::decode(known_transient_id.as_str()).expect("known transient bytes"),
                )]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Bool(false)));
        assert!(control
            .validated_peer_links
            .lock()
            .expect("validated peer links")
            .contains(&link_id));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 11, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        assert!(peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some(remote_propagation_hash.as_str())));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    remote_propagation_hash.as_str(),
                    known_transient_id.as_str(),
                )
                .expect("known offer mark"),
            "known offered payloads should be marked as already received from the offering peer"
        );
        daemon
            .record_propagation_offer_peer(remote_propagation_hash.as_str())
            .expect("admit peer after offer");
        let peers = daemon
            .handle_rpc(RpcRequest { id: 12, method: "list_peers".to_string(), params: None })
            .expect("list admitted peer")
            .result
            .expect("list admitted peer result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(remote_propagation_hash.as_str()))
            .expect("admitted peer row");
        assert_eq!(row["messages"]["handled_ids"], json!([known_transient_id]));
        assert_eq!(row["messages"]["unhandled_ids"], json!([]));
    }

    #[test]
    fn offer_request_rejects_throttled_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let remote_propagation_hash =
            hex::encode(propagation_destination_hash_for_identity(&remote_identity));
        daemon.throttle_propagation_peer_for_invalid_stamp(remote_propagation_hash.as_str());
        let offered = [0xBB; 32];
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let response = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(response, ControlResponse::Code(0xF6)));
    }

    #[test]
    fn offer_request_repeated_valid_offer_is_throttled_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 10,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "peering_cost": 1,
                })),
            })
            .expect("enable propagation");
        let local_identity_hash = [0x11; 16];
        let remote_private =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let remote_identity = *remote_private.as_identity();
        let mut peering_id = Vec::with_capacity(32);
        peering_id.extend_from_slice(local_identity_hash.as_slice());
        peering_id.extend_from_slice(remote_identity.address_hash.as_slice());
        let peering_key = generate_peering_key(peering_id.as_slice(), 1).expect("peering key");
        let offered = [0xBB; 32];
        let other_offered = [0xBC; 32];
        let control = PropagationControlContext {
            enabled: true,
            local_identity_hash,
            propagation_destination_hash_hex: Some("propagation".to_string()),
            control_destination_hash_hex: Some("control".to_string()),
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: test_validated_peer_links(),
            identified_peer_links: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        let first = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );
        let second = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key.clone()),
                rmpv::Value::Array(vec![rmpv::Value::Binary(offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );
        let different_offer = handle_offer_request(
            &daemon,
            &control,
            &test_link_id(),
            &remote_identity,
            Some(rmpv::Value::Array(vec![
                rmpv::Value::Binary(peering_key),
                rmpv::Value::Array(vec![rmpv::Value::Binary(other_offered.to_vec())]),
            ])),
            0xF1,
            0xF3,
            0xF4,
            0xF6,
        );

        assert!(matches!(first, ControlResponse::Bool(true)));
        assert!(matches!(second, ControlResponse::Code(0xF6)));
        assert!(matches!(different_offer, ControlResponse::Code(0xF6)));
    }
