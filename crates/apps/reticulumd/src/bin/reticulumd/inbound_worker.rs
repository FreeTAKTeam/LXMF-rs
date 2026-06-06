use super::bootstrap::PropagationControlContext;
use super::bridge_helpers::diagnostics_enabled;
use super::outbound_resources::{take_outbound_resource_tracking, OutboundResourceMap};
#[path = "inbound_control.rs"]
mod control;
#[path = "inbound_delivery_events.rs"]
mod delivery_events;
#[path = "inbound_propagation.rs"]
mod propagation;
#[path = "inbound_routing.rs"]
mod routing;
use reticulum_daemon::receipt_bridge::ReceiptEvent;
use rns_rpc::{RpcDaemon, RpcRequest};
use rns_transport::destination::link::{Link, LinkEvent};
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::{AddressHash, Hash};
use rns_transport::identity::{DecryptIdentity, Identity};
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::Transport;
use routing::InboundLxmfDestination;
use serde_json::{json, Value};
use sha2::Digest;
use std::sync::Arc;

pub(super) fn spawn_inbound_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
    receipt_tx: tokio::sync::mpsc::Sender<ReceiptEvent>,
    outbound_resource_map: OutboundResourceMap,
) {
    if control.enabled {
        control::spawn_control_worker(daemon.clone(), transport.clone(), control.clone());
    }
    let resource_control = control.clone();
    spawn_packet_inbound_worker(daemon.clone(), transport.clone(), control);
    tokio::spawn(async move {
        let mut rx = transport.resource_events();
        loop {
            if let Ok(event) = rx.recv().await {
                match event.kind {
                    ResourceEventKind::Complete(complete) => {
                        if let Some(destination) = routing::resolve_resource_destination(
                            transport.as_ref(),
                            &event.link_id,
                        )
                        .await
                        {
                            match destination {
                                InboundLxmfDestination::Delivery(destination) => {
                                    delivery_events::accept_delivery_resource(
                                        daemon.as_ref(),
                                        transport.as_ref(),
                                        destination,
                                        &complete.data,
                                    )
                                    .await;
                                }
                                InboundLxmfDestination::Propagation => {
                                    let remote_peer = remote_propagation_peer_for_link(
                                        transport.as_ref(),
                                        &event.link_id,
                                    )
                                    .await;
                                    let peer_link_validated = resource_control
                                        .validated_peer_links
                                        .lock()
                                        .ok()
                                        .is_some_and(|guard| guard.contains(&event.link_id));
                                    if let Err(error) =
                                        propagation::ingest_propagation_resource_from_peer(
                                            daemon.as_ref(),
                                            &complete.data,
                                            resource_control.delivery_destination.as_ref(),
                                            remote_peer.as_deref(),
                                            peer_link_validated,
                                        )
                                        .await
                                    {
                                        if diagnostics_enabled() {
                                            log::debug!(
                                                "[daemon-rx] dropping inbound propagation resource: {}",
                                                error
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    ResourceEventKind::OutboundComplete => {
                        handle_outbound_resource_completion(
                            daemon.as_ref(),
                            &outbound_resource_map,
                            &receipt_tx,
                            &event.hash,
                        );
                    }
                    ResourceEventKind::OutboundFailed => {
                        handle_outbound_resource_failure(
                            daemon.as_ref(),
                            &outbound_resource_map,
                            &receipt_tx,
                            &event.hash,
                        );
                    }
                    ResourceEventKind::OutboundCancelled => {
                        let resource_hash_hex = hex::encode(event.hash.as_slice());
                        let _ = take_outbound_resource_tracking(
                            &outbound_resource_map,
                            resource_hash_hex.as_str(),
                        );
                    }
                    ResourceEventKind::Progress(_) => {}
                }
            }
        }
    });
}

async fn remote_propagation_peer_for_link(
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<String> {
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        return Some(propagation_destination_hash_for_identity(guard.peer_identity()));
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        return Some(propagation_destination_hash_for_identity(guard.peer_identity()));
    }
    None
}

fn propagation_destination_hash_for_identity(identity: &Identity) -> String {
    let name = DestinationName::new("lxmf", "propagation");
    let hash = sha2::Sha256::new()
        .chain_update(name.as_name_hash_slice())
        .chain_update(identity.address_hash.as_slice())
        .finalize();
    hex::encode(&hash[..16])
}

fn handle_outbound_resource_completion(
    daemon: &RpcDaemon,
    outbound_resource_map: &OutboundResourceMap,
    receipt_tx: &tokio::sync::mpsc::Sender<ReceiptEvent>,
    resource_hash: &Hash,
) {
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    if let Some(tracking) =
        take_outbound_resource_tracking(outbound_resource_map, resource_hash_hex.as_str())
    {
        daemon.record_outbound_peer_sent(&tracking.peer, tracking.bytes);
        let _ = receipt_tx.try_send(ReceiptEvent {
            message_id: tracking.message_id,
            status: tracking.sent_status,
        });
    }
}

fn handle_outbound_resource_failure(
    daemon: &RpcDaemon,
    outbound_resource_map: &OutboundResourceMap,
    receipt_tx: &tokio::sync::mpsc::Sender<ReceiptEvent>,
    resource_hash: &Hash,
) {
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    if let Some(tracking) =
        take_outbound_resource_tracking(outbound_resource_map, resource_hash_hex.as_str())
    {
        daemon.record_outbound_peer_activity(&tracking.peer, tracking.bytes, false);
        let _ = receipt_tx.try_send(ReceiptEvent {
            message_id: tracking.message_id,
            status: "failed: resource transfer timed out".to_string(),
        });
    }
}

fn spawn_packet_inbound_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
) {
    let daemon_inbound = daemon;
    let inbound_transport = transport;
    tokio::spawn(async move {
        let local_delivery_destination =
            routing::local_delivery_destination_hash(control.delivery_destination.as_ref()).await;
        let mut rx = inbound_transport.received_data_events();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if routing::should_skip_control_payload(&event, &control) {
                        continue;
                    }
                    let data = event.data.as_slice();
                    let raw_destination_hex = hex::encode(event.destination.as_slice());
                    let Some(resolved_destination) = routing::resolve_packet_destination(
                        inbound_transport.as_ref(),
                        &control,
                        &event.destination,
                        event.payload_mode,
                        local_delivery_destination,
                    )
                    .await
                    else {
                        if diagnostics_enabled() {
                            log::debug!(
                                "[daemon-rx] skipping unresolved full-wire payload: dst={} len={} ctx={:?}",
                                raw_destination_hex,
                                data.len(),
                                event.context
                            );
                        }
                        continue;
                    };

                    delivery_events::log_resolved_packet(
                        &raw_destination_hex,
                        resolved_destination,
                        event.payload_mode,
                        event.ratchet_used,
                        data,
                    );

                    match resolved_destination {
                        InboundLxmfDestination::Propagation => {
                            if let Err(error) = propagation::ingest_propagation_envelope(
                                daemon_inbound.as_ref(),
                                data,
                                control.delivery_destination.as_ref(),
                            )
                            .await
                            {
                                if diagnostics_enabled() {
                                    log::debug!(
                                        "[daemon-rx] dropping inbound propagation payload: dst={} error={}",
                                        raw_destination_hex, error
                                    );
                                }
                            }
                            continue;
                        }
                        InboundLxmfDestination::Delivery(destination) => {
                            delivery_events::accept_delivery_packet(
                                daemon_inbound.as_ref(),
                                inbound_transport.as_ref(),
                                &raw_destination_hex,
                                destination,
                                data,
                                event.payload_mode,
                            )
                            .await;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    if diagnostics_enabled() {
                        log::debug!(
                            "[daemon-rx] received-data channel lagged; skipped {} events",
                            skipped
                        );
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
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

    #[tokio::test]
    async fn inbound_propagation_accepts_stamp_within_flexibility_window() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 41,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 3,
                    "stamp_cost_flexibility": 2,
                })),
            })
            .expect("enable propagation");
        let lxm_data = [0x43_u8; 113];
        let transient = stamped_propagation_payload_with_value_range(&lxm_data, 1, 3);
        let transient_id = hex::encode(Sha256::digest(lxm_data));
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![transient])).expect("propagation envelope");

        let ingested =
            ingest_propagation_envelope(&daemon, &envelope, None).await.expect("ingest envelope");
        assert_eq!(ingested, 1);

        let fetched = daemon
            .handle_rpc(RpcRequest {
                id: 42,
                method: "propagation_fetch".to_string(),
                params: Some(serde_json::json!({ "transient_id": transient_id })),
            })
            .expect("fetch propagation payload")
            .result
            .expect("fetch result");
        assert_eq!(fetched["payload_hex"].as_str(), Some(hex::encode(lxm_data).as_str()));
    }

    #[tokio::test]
    async fn propagation_envelope_does_not_decode_as_normal_lxmf_delivery() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 5,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 1,
                })),
            })
            .expect("enable propagation");
        let transient = stamped_propagation_payload(&[0x42_u8; 113], 1);
        let envelope =
            rmp_serde::to_vec(&(1.0_f64, vec![transient])).expect("propagation envelope");
        let destination = [0x22_u8; 16];

        assert!(inbound_delivery::decode_inbound_payload(
            destination,
            &envelope,
            lxmf::inbound_decode::InboundPayloadMode::FullWire,
        )
        .is_none());
        assert!(ingest_propagation_envelope(&daemon, &envelope, None).await.is_ok());
    }

    #[tokio::test]
    async fn local_propagation_payload_is_decrypted_and_accepted() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 39,
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

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "propagated title",
            "propagated content",
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
            .handle_rpc(RpcRequest { id: 40, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["direction"].as_str(), Some("in"));
        assert_eq!(items[0]["destination"].as_str(), Some(hex::encode(destination_hash).as_str()));
        assert_eq!(items[0]["title"].as_str(), Some("propagated title"));
        assert_eq!(items[0]["content"].as_str(), Some("propagated content"));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_checked"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_valid"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_target_cost"], json!(1));
        assert!(items[0]["fields"]["_lxmf"]["propagation_stamp_value"]
            .as_u64()
            .is_some_and(|value| value >= 1));
    }

    #[tokio::test]
    async fn local_propagation_payload_records_stamp_inside_flexibility_window() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 44,
                method: "propagation_enable".to_string(),
                params: Some(serde_json::json!({
                    "enabled": true,
                    "target_cost": 3,
                    "stamp_cost_flexibility": 2,
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

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "flex propagated title",
            "flex propagated content",
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
            let (transient, _) = message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient");
            let stamped = stamped_propagation_payload_with_value_range(&transient, 1, 3);
            rmp_serde::to_vec(&(1.0_f64, vec![stamped])).expect("propagation envelope")
        };

        let ingested = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
            .await
            .expect("ingest propagation envelope");
        assert_eq!(ingested, 1);

        let messages = daemon
            .handle_rpc(RpcRequest { id: 45, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        let items = messages["messages"].as_array().expect("message items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"].as_str(), Some("flex propagated title"));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_checked"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_valid"], json!(true));
        assert_eq!(items[0]["fields"]["_lxmf"]["propagation_stamp_target_cost"], json!(1));
        assert!(items[0]["fields"]["_lxmf"]["propagation_stamp_value"]
            .as_u64()
            .is_some_and(|value| (1..3).contains(&value)));
    }

    #[tokio::test]
    async fn inbound_peer_propagation_local_delivery_counts_source_peer_like_python() {
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
        let propagation_peer = hex::encode([0x7F_u8; 16]);
        daemon
            .handle_rpc(RpcRequest {
                id: 47,
                method: "peer_sync".to_string(),
                params: Some(serde_json::json!({ "peer": propagation_peer })),
            })
            .expect("seed propagation peer");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "peer local propagated title",
            "peer local propagated content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let (envelope, transient_id, transient_len) = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            let (transient, transient_id) = message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient");
            let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
            (
                WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
                    .expect("propagation envelope"),
                hex::encode(transient_id),
                transient.len(),
            )
        };

        let ingested = ingest_propagation_envelope_from_peer(
            &daemon,
            &envelope,
            Some(&delivery_destination),
            Some(&propagation_peer),
        )
        .await
        .expect("ingest peer propagation envelope");
        assert_eq!(ingested, 1);

        let peer = peer_row(&daemon, propagation_peer.as_str(), 48);
        assert_eq!(peer["messages"]["incoming"].as_u64(), Some(1));
        assert_eq!(peer["rx_bytes"].as_u64(), Some(transient_len as u64));
        assert!(
            daemon
                .has_peer_completed_propagation_mark(
                    propagation_peer.as_str(),
                    transient_id.as_str()
                )
                .expect("completed propagation mark lookup"),
            "locally delivered peer propagation payloads should still mark the source peer handled"
        );

        let status = daemon
            .handle_rpc(RpcRequest {
                id: 49,
                method: "propagation_status".to_string(),
                params: None,
            })
            .expect("propagation status")
            .result
            .expect("propagation status result");
        assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn duplicate_local_propagation_payload_does_not_update_peer_activity_like_python() {
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
        let source_hex = hex::encode(source_hash);
        daemon.accept_announce(source_hex.clone(), 1).expect("accept source announce");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "duplicate propagated title",
            "duplicate propagated content",
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

        let first_ingested =
            ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
                .await
                .expect("first ingest propagation envelope");
        assert_eq!(first_ingested, 1);
        let after_first = peer_row(&daemon, source_hex.as_str(), 42);
        assert_eq!(after_first["rx_bytes"].as_u64(), Some(wire.len() as u64));
        assert_eq!(after_first["messages"]["incoming"].as_u64(), Some(1));

        let second_ingested =
            ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
                .await
                .expect("second ingest propagation envelope");
        assert_eq!(second_ingested, 1);

        let messages = daemon
            .handle_rpc(RpcRequest { id: 43, method: "list_messages".to_string(), params: None })
            .expect("list messages")
            .result
            .expect("list messages result");
        assert_eq!(messages["messages"].as_array().map(Vec::len), Some(1));
        let after_second = peer_row(&daemon, source_hex.as_str(), 44);
        assert_eq!(after_second["rx_bytes"].as_u64(), Some(wire.len() as u64));
        assert_eq!(after_second["messages"]["incoming"].as_u64(), Some(1));
    }

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
}
