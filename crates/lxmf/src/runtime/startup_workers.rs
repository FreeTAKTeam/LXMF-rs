use super::announce_helpers::{
    lxmf_aspect_from_name_hash, parse_peer_name_from_app_data, update_peer_announce_meta,
};
use super::inbound_helpers::{annotate_inbound_transport_metadata, decode_inbound_payload};
use super::peer_cache::persist_peer_identity_cache;
use super::{
    handle_receipt_event, resolve_link_destination, AnnounceBridge, EmbeddedTransportBridge,
    PeerAnnounceMeta, PeerCrypto, ReceiptEvent, RpcDaemon, STARTUP_ANNOUNCE_BURST_DELAYS_SECS,
};
#[cfg(reticulum_api_v2)]
use crate::helpers::{pn_peering_cost_from_app_data, pn_stamp_cost_flexibility_from_app_data};
use crate::inbound_decode::InboundPayloadMode;
use reticulum::resource::ResourceEventKind;
use reticulum::transport::Transport;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;

pub(super) fn spawn_receipt_worker(
    daemon: Rc<RpcDaemon>,
    mut receipt_rx: UnboundedReceiver<ReceiptEvent>,
    shutdown_tx: &watch::Sender<bool>,
) {
    let daemon_receipts = daemon;
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::task::spawn_local(async move {
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                event = receipt_rx.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    let _ = handle_receipt_event(&daemon_receipts, event);
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_transport_workers(
    transport: Arc<Transport>,
    daemon: Rc<RpcDaemon>,
    receipt_tx: UnboundedSender<ReceiptEvent>,
    outbound_resource_map: Arc<Mutex<HashMap<String, String>>>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    peer_announce_meta: Arc<Mutex<HashMap<String, PeerAnnounceMeta>>>,
    peer_identity_cache_path: PathBuf,
    known_propagation_nodes: Arc<Mutex<HashSet<String>>>,
    shutdown_tx: &watch::Sender<bool>,
) {
    let daemon_inbound = daemon.clone();
    let inbound_transport = transport.clone();
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::task::spawn_local(async move {
        let mut rx = inbound_transport.received_data_events();
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let data = event.data.as_slice();
                            let mut destination = [0u8; 16];
                            destination.copy_from_slice(event.destination.as_slice());
                            if let Some(mut record) =
                                decode_inbound_payload(
                                    destination,
                                    data,
                                    InboundPayloadMode::DestinationStripped,
                                )
                            {
                                annotate_inbound_transport_metadata(&mut record, &event);
                                let _ = daemon_inbound.accept_inbound(record);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    let daemon_resource_inbound = daemon.clone();
    let resource_transport = transport.clone();
    let resource_receipt_tx = receipt_tx;
    let resource_outbound_map = outbound_resource_map;
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::task::spawn_local(async move {
        let mut rx = resource_transport.resource_events();
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            match event.kind {
                                ResourceEventKind::Complete(complete) => {
                                    if let Some(destination) = resolve_link_destination(&resource_transport, &event.link_id).await {
                                        if let Some(record) = decode_inbound_payload(
                                            destination,
                                            &complete.data,
                                            InboundPayloadMode::DestinationStripped,
                                        ) {
                                            let _ = daemon_resource_inbound.accept_inbound(record);
                                        }
                                    }
                                }
                                ResourceEventKind::OutboundComplete => {
                                    let resource_hash_hex = hex::encode(event.hash.as_slice());
                                    let message_id = resource_outbound_map
                                        .lock()
                                        .ok()
                                        .and_then(|mut guard| guard.remove(&resource_hash_hex));
                                    if let Some(message_id) = message_id {
                                        let _ = resource_receipt_tx.send(ReceiptEvent {
                                            message_id,
                                            status: "sent: link resource".to_string(),
                                        });
                                    }
                                }
                                ResourceEventKind::Progress(_) => {}
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    let daemon_announce = daemon;
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::task::spawn_local(async move {
        let mut rx = transport.recv_announces().await;
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let dest = event.destination.lock().await;
                            let peer = hex::encode(dest.desc.address_hash.as_slice());
                            let identity = dest.desc.identity;
                            let app_data = event.app_data.as_slice();
                            let (peer_name, peer_name_source) = parse_peer_name_from_app_data(app_data)
                                .map(|(name, source)| (Some(name), Some(source)))
                                .unwrap_or((None, None));

                            peer_crypto
                                .lock()
                                .expect("peer map")
                                .insert(peer.clone(), PeerCrypto { identity });
                            persist_peer_identity_cache(
                                &peer_crypto,
                                &peer_identity_cache_path,
                            );
                            update_peer_announce_meta(
                                &peer_announce_meta,
                                &peer,
                                app_data,
                            );

                            let timestamp = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|value| value.as_secs() as i64)
                                .unwrap_or(0);
                            let aspect =
                                lxmf_aspect_from_name_hash(dest.desc.name.as_name_hash_slice());
                            if aspect.as_deref() == Some("lxmf.propagation") {
                                if let Ok(mut nodes) = known_propagation_nodes.lock() {
                                    nodes.insert(peer.clone());
                                }
                            }
                            #[cfg(reticulum_api_v2)]
                            {
                                let app_data_hex = (!app_data.is_empty())
                                    .then(|| hex::encode(app_data));
                                let hops = Some(u32::from(event.hops));
                                let interface =
                                    Some(hex::encode(event.interface.as_slice()));

                                let _ = daemon_announce.accept_announce_with_metadata(
                                    peer,
                                    timestamp,
                                    peer_name,
                                    peer_name_source,
                                    app_data_hex,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    Some(pn_stamp_cost_flexibility_from_app_data(app_data)),
                                    Some(pn_peering_cost_from_app_data(app_data)),
                                    aspect,
                                    hops,
                                    interface,
                                    None,
                                    None,
                                    None,
                                );
                            }

                            #[cfg(not(reticulum_api_v2))]
                            {
                                let _ = daemon_announce.accept_announce_with_details(
                                    peer,
                                    timestamp,
                                    peer_name,
                                    peer_name_source,
                                );
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });
}

pub(super) fn spawn_startup_announce_burst(bridge: Arc<EmbeddedTransportBridge>) {
    tokio::task::spawn_local(async move {
        // Emit a short announce burst after startup to improve cross-client
        // discovery when peers/interfaces come online slightly later.
        for delay_secs in STARTUP_ANNOUNCE_BURST_DELAYS_SECS {
            tokio::time::sleep(std::time::Duration::from_secs(*delay_secs)).await;
            let _ = bridge.announce_now();
        }
    });
}
