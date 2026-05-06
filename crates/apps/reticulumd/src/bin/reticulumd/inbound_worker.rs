use super::bootstrap::PropagationControlContext;
use super::bridge_helpers::diagnostics_enabled;
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
use rns_transport::destination::SingleInputDestination;
use rns_transport::hash::AddressHash;
use rns_transport::identity::{DecryptIdentity, Identity};
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::Transport;
use routing::InboundLxmfDestination;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) const OUTBOUND_RESOURCE_SENT_STATUS: &str = "sent: link resource";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OutboundResourceTracking {
    pub(super) message_id: String,
    pub(super) peer: String,
    pub(super) bytes: usize,
    pub(super) sent_status: String,
}

pub(super) fn spawn_inbound_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
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
                                    if let Err(error) = propagation::ingest_propagation_envelope(
                                        daemon.as_ref(),
                                        &complete.data,
                                        resource_control.delivery_destination.as_ref(),
                                    )
                                    .await
                                    {
                                        if diagnostics_enabled() {
                                            eprintln!(
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
                        let resource_hash_hex = hex::encode(event.hash.as_slice());
                        if let Some(tracking) = take_outbound_resource_tracking(
                            &outbound_resource_map,
                            resource_hash_hex.as_str(),
                        ) {
                            daemon.record_outbound_peer_activity(
                                &tracking.peer,
                                tracking.bytes,
                                true,
                            );
                            let _ = receipt_tx.send(ReceiptEvent {
                                message_id: tracking.message_id,
                                status: tracking.sent_status,
                            });
                        }
                    }
                    ResourceEventKind::Progress(_) => {}
                }
            }
        }
    });
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
                            eprintln!(
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
                                    eprintln!(
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
                        eprintln!(
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

pub(super) fn track_outbound_resource(
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    resource_hash_hex: String,
    tracking: OutboundResourceTracking,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.insert(resource_hash_hex, tracking);
    }
}

pub(super) fn take_outbound_resource_tracking(
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    resource_hash_hex: &str,
) -> Option<OutboundResourceTracking> {
    outbound_resource_map.lock().ok().and_then(|mut guard| guard.remove(resource_hash_hex))
}

pub(super) fn prune_outbound_resource_mappings_for_message(
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    message_id: &str,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.retain(|_, tracking| tracking.message_id != message_id);
    }
}

#[cfg(test)]
mod tests {
    use super::propagation::ingest_propagation_envelope;
    use hkdf::Hkdf;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::inbound_delivery;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use rns_rpc::{RpcDaemon, RpcRequest};
    use rns_transport::destination::{DestinationName, SingleInputDestination};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{to_core_identity, to_core_private_identity};
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
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
            WireMessage::unpack(&wire)
                .expect("wire unpack")
                .pack_propagation_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    1.0,
                    OsRng,
                )
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
    }

    fn stamped_propagation_payload(lxm_data: &[u8], target_cost: u32) -> Vec<u8> {
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
            if value >= target_cost {
                break;
            }
            nonce = nonce.wrapping_add(1);
        }

        let mut transient = lxm_data.to_vec();
        transient.extend_from_slice(&stamp);
        transient
    }
}
