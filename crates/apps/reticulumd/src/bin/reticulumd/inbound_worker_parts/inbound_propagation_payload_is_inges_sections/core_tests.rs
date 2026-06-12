    use super::delivery_events;

    use super::propagation::{
        ingest_propagation_envelope, ingest_propagation_envelope_from_peer,
        ingest_propagation_resource_from_peer,
    };

    use hkdf::Hkdf;

    use lxmf::WireMessage;

    use rand_core::OsRng;

    use reticulum_daemon::inbound_delivery;

    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;

    use reticulum_daemon::lxmf_stamps::generate_propagation_stamp;

    use rns_rpc::{RpcDaemon, RpcRequest};

    use rns_transport::destination::{DestinationName, SingleInputDestination};

    use rns_transport::hash::Hash;

    use rns_transport::identity::PrivateIdentity;

    use rns_transport::identity_bridge::{
        to_core_identity, to_core_private_identity, to_transport_private_identity,
    };

    use rns_transport::transport::{ReceivedPayloadMode, Transport, TransportConfig};

    use serde_json::json;

    use sha2::{Digest, Sha256};

    use std::collections::HashMap;

    use std::sync::{Arc, Mutex};

    use tokio::sync::Mutex as TokioMutex;

    #[tokio::test]
    async fn inbound_propagation_payload_is_ingested_and_counted() {
        let daemon = RpcDaemon::test_instance();
        let payload = b"plain-propagation-payload".to_vec();
        let transient_id = hex::encode(Sha256::digest(&payload));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![payload.clone()])).expect("propagation envelope");

        let ingested =
            ingest_propagation_envelope(&daemon, &envelope, None).await.expect("ingest envelope");
        assert_eq!(ingested, 1);

        let fetched = daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_fetch".to_string(),
                params: Some(serde_json::json!({ "transient_id": transient_id })),
            })
            .expect("fetch propagation payload")
            .result
            .expect("fetch result");
        assert_eq!(fetched["payload_hex"].as_str(), Some(hex::encode(&payload).as_str()));

        let status = daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    }

    #[test]
    fn outbound_resource_failure_event_marks_tracking_failed() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "propagation_enable".to_string(),
                params: Some(json!({
                    "enabled": true,
                    "static_peers": ["peer-resource-timeout"],
                })),
            })
            .expect("enable static peer");
        let resource_hash = Hash::new_from_slice(&[0x51; 32]);
        let resource_hash_hex = hex::encode(resource_hash.as_slice());
        let map = Arc::new(Mutex::new(HashMap::new()));
        super::super::outbound_resources::track_outbound_resource(
            &map,
            resource_hash_hex.clone(),
            super::super::outbound_resources::OutboundResourceTracking {
                message_id: "resource-timeout-message".to_string(),
                peer: "peer-resource-timeout".to_string(),
                bytes: 512,
                sent_status: "sent: link resource".to_string(),
            },
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        super::handle_outbound_resource_failure(&daemon, &map, &tx, &resource_hash);

        assert!(super::super::outbound_resources::take_outbound_resource_tracking(
            &map,
            resource_hash_hex.as_str()
        )
        .is_none());
        let event = rx.try_recv().expect("failed receipt event");
        assert_eq!(event.message_id, "resource-timeout-message");
        assert_eq!(event.status, "failed: resource transfer timed out");
        let peers = daemon
            .handle_rpc(RpcRequest { id: 2, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
        assert_eq!(row["tx_bytes"].as_u64(), Some(512));
        assert_eq!(row["alive"].as_bool(), Some(false));
        assert_eq!(row["sync_backoff"].as_u64(), Some(12 * 60));
    }

    #[tokio::test]
    async fn inbound_propagation_invalid_entry_is_rejected() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 3,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                    "stamp_cost_flexibility": 0,
                })),
            })
            .expect("enable propagation");
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![b"unstamped-propagation-payload".to_vec()]))
                .expect("propagation envelope");

        let err = ingest_propagation_envelope(&daemon, &envelope, None)
            .await
            .expect_err("invalid propagation envelope should be rejected");
        assert!(err.to_string().contains("invalid propagation stamp"));

        let status = daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn inbound_propagation_invalid_peer_stamp_throttles_peer_like_python() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                    "stamp_cost_flexibility": 0,
                })),
            })
            .expect("enable propagation");
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![b"unstamped-peer-propagation-payload".to_vec()]))
                .expect("propagation envelope");
        let peer = hex::encode([0x77_u8; 16]);

        let err = ingest_propagation_envelope_from_peer(&daemon, &envelope, None, Some(&peer))
            .await
            .expect_err("invalid peer propagation envelope should be rejected");

        assert!(err.to_string().contains("invalid propagation stamp"));
        assert!(daemon.propagation_peer_is_throttled(peer.as_str()));
    }

    #[tokio::test]
    async fn inbound_peer_propagation_preserves_valid_messages_when_transfer_has_invalid_stamp_like_python(
    ) {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 44,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                    "stamp_cost_flexibility": 0,
                })),
            })
            .expect("enable propagation");
        let valid_lxm_data = [0x52_u8; 113];
        let valid_transient = stamped_propagation_payload(&valid_lxm_data, 1);
        let valid_transient_id = hex::encode(Sha256::digest(valid_lxm_data));
        let invalid_transient = b"unstamped-peer-propagation-payload".to_vec();
        let invalid_transient_id = hex::encode(Sha256::digest(&invalid_transient));
        let envelope = rmp_serde::to_vec(&(1.0_f64, vec![invalid_transient, valid_transient]))
            .expect("propagation envelope");
        let peer = hex::encode([0x7A_u8; 16]);

        let err = ingest_propagation_envelope_from_peer(&daemon, &envelope, None, Some(&peer))
            .await
            .expect_err("mixed-stamp peer resource should reject the transfer");

        assert!(err.to_string().contains("invalid propagation stamp"));
        assert!(daemon.propagation_peer_is_throttled(peer.as_str()));
        assert!(
            daemon.has_propagation_payload(valid_transient_id.as_str()),
            "valid entries in a mixed peer transfer should still be ingested"
        );
        assert!(!daemon.has_propagation_payload(invalid_transient_id.as_str()));
    }

    #[tokio::test]
    async fn inbound_peer_propagation_marks_source_handled_and_queues_other_peers_like_python() {
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
        let source_peer = hex::encode([0x7B_u8; 16]);
        let relay_peer = hex::encode([0x7C_u8; 16]);
        for (id, peer) in [(47, &source_peer), (48, &relay_peer)] {
            daemon
                .handle_rpc(RpcRequest {
                    id,
                    method: "peer_sync".to_string(),
                    params: Some(serde_json::json!({ "peer": peer })),
                })
                .expect("seed propagation peer");
        }
        let lxm_data = [0x53_u8; 113];
        let transient = stamped_propagation_payload(&lxm_data, 1);
        let transient_id = hex::encode(Sha256::digest(lxm_data));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![transient])).expect("propagation envelope");

        let ingested =
            ingest_propagation_envelope_from_peer(&daemon, &envelope, None, Some(&source_peer))
                .await
                .expect("ingest peer propagation envelope");

        assert_eq!(ingested, 1);
        let source_row = peer_row(&daemon, source_peer.as_str(), 49);
        assert_eq!(
            source_row["messages"]["handled_ids"].as_array().expect("source handled ids"),
            &[serde_json::json!(transient_id.as_str())]
        );
        assert!(source_row["messages"]["unhandled_ids"]
            .as_array()
            .expect("source unhandled ids")
            .is_empty());
        assert_eq!(source_row["rx_bytes"].as_u64(), Some(lxm_data.len() as u64));
        assert_eq!(source_row["messages"]["incoming"].as_u64(), Some(1));
        let relay_row = peer_row(&daemon, relay_peer.as_str(), 50);
        assert_eq!(
            relay_row["messages"]["unhandled_ids"].as_array().expect("relay unhandled ids"),
            &[serde_json::json!(transient_id.as_str())]
        );
    }

    #[tokio::test]
    async fn inbound_unpeered_propagation_counts_unpeered_sender_and_queues_active_peers_like_python(
    ) {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 51,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                    "stamp_cost_flexibility": 0,
                })),
            })
            .expect("enable propagation");
        let unpeered_source = hex::encode([0x7D_u8; 16]);
        let relay_peer = hex::encode([0x7E_u8; 16]);
        daemon
            .handle_rpc(RpcRequest {
                id: 52,
                method: "peer_sync".to_string(),
                params: Some(serde_json::json!({ "peer": relay_peer })),
            })
            .expect("seed relay peer");
        let lxm_data = [0x54_u8; 113];
        let transient = stamped_propagation_payload(&lxm_data, 1);
        let transient_id = hex::encode(Sha256::digest(lxm_data));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![transient])).expect("propagation envelope");

        let ingested =
            ingest_propagation_envelope_from_peer(&daemon, &envelope, None, Some(&unpeered_source))
                .await
                .expect("ingest unpeered propagation envelope");

        assert_eq!(ingested, 1);
        let status = daemon
            .handle_rpc(RpcRequest {
                id: 53,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(status["propagation"]["unpeered_propagation_incoming"].as_u64(), Some(1));
        assert_eq!(
            status["propagation"]["unpeered_propagation_rx_bytes"].as_u64(),
            Some(lxm_data.len() as u64)
        );
        assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
        let peers = daemon
            .handle_rpc(RpcRequest { id: 54, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let rows = peers["peers"].as_array().expect("peer rows");
        assert!(
            rows.iter().all(|row| row["peer"].as_str() != Some(unpeered_source.as_str())),
            "unpeered sender should not be promoted to an active peer"
        );
        let relay_row = peer_row(&daemon, relay_peer.as_str(), 55);
        assert_eq!(
            relay_row["messages"]["unhandled_ids"].as_array().expect("relay unhandled ids"),
            &[serde_json::json!(transient_id.as_str())]
        );
    }

    #[tokio::test]
    async fn inbound_peer_propagation_rejects_multi_message_without_validated_link_like_python() {
        let daemon = RpcDaemon::test_instance();
        let first = b"unvalidated-peer-first".to_vec();
        let second = b"unvalidated-peer-second".to_vec();
        let first_id = hex::encode(Sha256::digest(&first));
        let second_id = hex::encode(Sha256::digest(&second));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![first, second])).expect("propagation envelope");
        let peer = hex::encode([0x78_u8; 16]);

        let err =
            ingest_propagation_resource_from_peer(&daemon, &envelope, None, Some(&peer), false)
                .await
                .expect_err("unvalidated peer resource should reject multi-message transfer");

        assert!(err.to_string().contains("valid peering key"));
        assert!(!daemon.has_propagation_payload(first_id.as_str()));
        assert!(!daemon.has_propagation_payload(second_id.as_str()));
    }

    #[tokio::test]
    async fn inbound_client_packet_propagation_accepts_multi_message_like_python() {
        let daemon = RpcDaemon::test_instance();
        let first = b"unvalidated-client-first".to_vec();
        let second = b"unvalidated-client-second".to_vec();
        let first_id = hex::encode(Sha256::digest(&first));
        let second_id = hex::encode(Sha256::digest(&second));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![first, second])).expect("propagation envelope");

        let ingested = ingest_propagation_envelope(&daemon, &envelope, None)
            .await
            .expect("multi-message packet propagation should be accepted");

        assert_eq!(ingested, 2);
        assert!(daemon.has_propagation_payload(first_id.as_str()));
        assert!(daemon.has_propagation_payload(second_id.as_str()));
    }

    #[tokio::test]
    async fn inbound_client_resource_rejects_multi_message_without_validated_link_like_python() {
        let daemon = RpcDaemon::test_instance();
        let first = b"unvalidated-client-resource-first".to_vec();
        let second = b"unvalidated-client-resource-second".to_vec();
        let first_id = hex::encode(Sha256::digest(&first));
        let second_id = hex::encode(Sha256::digest(&second));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![first, second])).expect("propagation envelope");

        let err = ingest_propagation_resource_from_peer(&daemon, &envelope, None, None, false)
            .await
            .expect_err("unvalidated client resource should reject multi-message transfer");

        assert!(err.to_string().contains("valid peering key"));
        assert!(!daemon.has_propagation_payload(first_id.as_str()));
        assert!(!daemon.has_propagation_payload(second_id.as_str()));
    }

    #[tokio::test]
    async fn inbound_peer_propagation_accepts_multi_message_with_validated_link_like_python() {
        let daemon = RpcDaemon::test_instance();
        let first = b"validated-peer-first".to_vec();
        let second = b"validated-peer-second".to_vec();
        let first_id = hex::encode(Sha256::digest(&first));
        let second_id = hex::encode(Sha256::digest(&second));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![first, second])).expect("propagation envelope");
        let peer = hex::encode([0x79_u8; 16]);

        let ingested =
            ingest_propagation_resource_from_peer(&daemon, &envelope, None, Some(&peer), true)
                .await
                .expect("validated peer resource should accept multi-message transfer");

        assert_eq!(ingested, 2);
        assert!(daemon.has_propagation_payload(first_id.as_str()));
        assert!(daemon.has_propagation_payload(second_id.as_str()));
    }
