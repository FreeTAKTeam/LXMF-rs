use reticulum_daemon::receipt_bridge::ReceiptEvent;

use crate::bridge::emit_receipt_event;
use crate::direct_backchannel::DirectBackchannelLinks;

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
    direct_backchannel_links: Option<DirectBackchannelLinks>,
    receipt_tx: tokio::sync::mpsc::Sender<ReceiptEvent>,
    outbound_resource_map: OutboundResourceMap,
) {
    if control.enabled {
        control::spawn_control_worker(daemon.clone(), transport.clone(), control.clone());
    }
    let resource_control = control.clone();
    spawn_packet_inbound_worker(
        daemon.clone(),
        transport.clone(),
        control,
        direct_backchannel_links,
    );
    tokio::spawn(async move {
        let local_delivery_destination = routing::local_delivery_destination_hash(
            resource_control.delivery_destination.as_ref(),
        )
        .await;
        let mut rx = transport.resource_events();
        loop {
            if let Ok(event) = rx.recv().await {
                match event.kind {
                    ResourceEventKind::Complete(complete) => {
                        if let Some(destination) = routing::resolve_resource_destination(
                            transport.as_ref(),
                            &event.link_id,
                            local_delivery_destination,
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
                                    if complete.is_request {
                                        match resource_request_id(&complete.request_id) {
                                            Some(request_id) => {
                                                if let Err(error) =
                                                    control::handle_resource_control_request(
                                                        daemon.as_ref(),
                                                        transport.as_ref(),
                                                        &resource_control,
                                                        &event.link_id,
                                                        &complete.data,
                                                        request_id,
                                                        true,
                                                    )
                                                    .await
                                                {
                                                    log::error!(
                                                        "[daemon-control] failed to handle propagation resource request link={} error={}",
                                                        event.link_id,
                                                        error
                                                    );
                                                }
                                            }
                                            None => {
                                                log::warn!(
                                                    "[daemon-control] ignoring propagation resource request with invalid request id link={}",
                                                    event.link_id
                                                );
                                            }
                                        }
                                        continue;
                                    }
                                    if complete.is_response {
                                        continue;
                                    }
                                    let remote_peer = remote_propagation_peer_for_link(
                                        transport.as_ref(),
                                        &event.link_id,
                                    )
                                    .await;
                                    let peer_link_validated =
                                        match resource_control.validated_peer_links.lock() {
                                            Ok(guard) => guard.contains(&event.link_id),
                                            Err(err) => {
                                                log::warn!(
                                                    "[daemon-rx] failed to read validated peer links for link={}: {err}",
                                                    hex::encode(event.link_id.as_slice())
                                                );
                                                false
                                            }
                                        };
                                    if let Err(error) =
                                        propagation::ingest_propagation_resource_from_peer_with_transport(
                                            daemon.as_ref(),
                                            &complete.data,
                                            resource_control.delivery_destination.as_ref(),
                                            remote_peer.as_deref(),
                                            peer_link_validated,
                                            Some(transport.as_ref()),
                                        )
                                        .await
                                    {
                                        log::debug!(
                                            "[daemon-rx] dropping inbound propagation resource: {}",
                                            error
                                        );
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
                        match take_outbound_resource_tracking(
                            &outbound_resource_map,
                            resource_hash_hex.as_str(),
                        ) {
                            Ok(_) => {}
                            Err("resource not tracked") => log::debug!(
                                "[daemon-rx] cancelled outbound resource already untracked hash={resource_hash_hex}"
                            ),
                            Err(error) => log::error!(
                                "[daemon-rx] failed to clear cancelled outbound resource hash={resource_hash_hex}: {error}"
                            ),
                        }
                    }
                    ResourceEventKind::InboundFailed(failure) => {
                        log::warn!(
                            "[daemon-rx] inbound resource failed link={} hash={} reason={} received={}/{}",
                            event.link_id,
                            event.hash,
                            failure.reason,
                            failure.progress.received_parts,
                            failure.progress.total_parts
                        );
                    }
                    ResourceEventKind::SegmentComplete(segment) => {
                        log::debug!(
                            "[daemon-rx] resource segment complete link={} original_hash={} segment={}/{}",
                            event.link_id,
                            segment.original_hash,
                            segment.segment_index,
                            segment.total_segments
                        );
                    }
                    ResourceEventKind::Progress(_) => {}
                }
            }
        }
    });
}

fn resource_request_id(request_id: &Option<Vec<u8>>) -> Option<[u8; 16]> {
    let bytes = request_id.as_ref()?;
    if bytes.len() != 16 {
        return None;
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(bytes.as_slice());
    Some(out)
}

async fn remote_propagation_peer_for_link(
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<String> {
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        let identity = guard.identified_peer_identity().unwrap_or_else(|| guard.peer_identity());
        return Some(propagation_destination_hash_for_identity(identity));
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        let identity = guard.identified_peer_identity().unwrap_or_else(|| guard.peer_identity());
        return Some(propagation_destination_hash_for_identity(identity));
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
    match take_outbound_resource_tracking(outbound_resource_map, resource_hash_hex.as_str()) {
        Ok(tracking) => {
            daemon.record_outbound_peer_sent(&tracking.peer, tracking.bytes);
            emit_receipt_event(
                receipt_tx,
                ReceiptEvent::new(tracking.message_id, tracking.sent_status)
                    .with_resource_hash(resource_hash_hex)
                    .with_peer(tracking.peer)
                    .with_delivery_kind("resource-complete")
                    .with_bytes(tracking.bytes),
            );
        }
        Err(err) => {
            log::warn!("[daemon-rx] outbound resource completion without tracking hash={}: {err}", resource_hash_hex);
        }
    }
}

fn handle_outbound_resource_failure(
    daemon: &RpcDaemon,
    outbound_resource_map: &OutboundResourceMap,
    receipt_tx: &tokio::sync::mpsc::Sender<ReceiptEvent>,
    resource_hash: &Hash,
) {
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    match take_outbound_resource_tracking(outbound_resource_map, resource_hash_hex.as_str()) {
        Ok(tracking) => {
            daemon.record_outbound_peer_activity(&tracking.peer, tracking.bytes, false);
            emit_receipt_event(
                receipt_tx,
                ReceiptEvent::new(tracking.message_id, "failed: resource transfer timed out")
                    .with_resource_hash(resource_hash_hex)
                    .with_peer(tracking.peer)
                    .with_delivery_kind("resource-failed")
                    .with_bytes(tracking.bytes),
            );
        }
        Err(err) => {
            log::warn!("[daemon-rx] outbound resource failure without tracking hash={}: {err}", resource_hash_hex);
        }
    }
}

fn spawn_packet_inbound_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
    direct_backchannel_links: Option<DirectBackchannelLinks>,
) {
    if let Some(backchannel_links) = direct_backchannel_links {
        link_identification::spawn_identified_peer_workers(transport.clone(), backchannel_links);
    }

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
                        log::debug!(
                            "[daemon-rx] skipping unresolved full-wire payload: dst={} len={} ctx={:?}",
                            raw_destination_hex,
                            data.len(),
                            event.context
                        );
                        continue;
                    };

                    if routing::should_skip_resolved_control_payload(
                        resolved_destination,
                        event.context,
                    ) {
                        continue;
                    }

                    delivery_events::log_resolved_packet(
                        &raw_destination_hex,
                        resolved_destination,
                        event.payload_mode,
                        event.ratchet_used,
                        data,
                    );

                    match resolved_destination {
                        InboundLxmfDestination::Propagation => {
                            if let Err(error) = propagation::ingest_propagation_envelope_with_transport(
                                daemon_inbound.as_ref(),
                                data,
                                control.delivery_destination.as_ref(),
                                Some(inbound_transport.as_ref()),
                            )
                            .await
                            {
                                log::debug!(
                                    "[daemon-rx] dropping inbound propagation payload: dst={} error={}",
                                    raw_destination_hex, error
                                );
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
                    log::debug!(
                        "[daemon-rx] received-data channel lagged; skipped {} events",
                        skipped
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
