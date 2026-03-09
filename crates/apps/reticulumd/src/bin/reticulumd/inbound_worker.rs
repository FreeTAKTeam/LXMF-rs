use super::bridge_helpers::{diagnostics_enabled, payload_preview};
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    decode_inbound_payload, decode_inbound_payload_with_diagnostics,
};
use reticulum_daemon::receipt_bridge::ReceiptEvent;
use rns_rpc::RpcDaemon;
use rns_transport::hash::AddressHash;
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{ReceivedPayloadMode, Transport};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn inbound_payload_mode(mode: ReceivedPayloadMode) -> InboundPayloadMode {
    match mode {
        ReceivedPayloadMode::FullWire => InboundPayloadMode::FullWire,
        ReceivedPayloadMode::DestinationStripped => InboundPayloadMode::DestinationStripped,
    }
}

pub(super) fn spawn_inbound_worker(
    daemon: Rc<RpcDaemon>,
    transport: Arc<Transport>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    outbound_resource_map: Arc<Mutex<HashMap<String, String>>>,
) {
    spawn_packet_inbound_worker(daemon.clone(), transport.clone());
    tokio::task::spawn_local(async move {
        let mut rx = transport.resource_events();
        loop {
            if let Ok(event) = rx.recv().await {
                match event.kind {
                    ResourceEventKind::Complete(complete) => {
                        if let Some(destination) =
                            resolve_link_destination(transport.as_ref(), &event.link_id).await
                        {
                            if let Some(record) = decode_inbound_payload(
                                destination,
                                &complete.data,
                                InboundPayloadMode::FullWire,
                            ) {
                                let _ = daemon.accept_inbound(record);
                            }
                        }
                    }
                    ResourceEventKind::OutboundComplete => {
                        let resource_hash_hex = hex::encode(event.hash.as_slice());
                        if let Some(message_id) = take_outbound_resource_message_id(
                            &outbound_resource_map,
                            resource_hash_hex.as_str(),
                        ) {
                            let _ = receipt_tx
                                .send(ReceiptEvent { message_id, status: "delivered".to_string() });
                        }
                    }
                    ResourceEventKind::Progress(_) => {}
                }
            }
        }
    });
}

fn spawn_packet_inbound_worker(daemon: Rc<RpcDaemon>, transport: Arc<Transport>) {
    let daemon_inbound = daemon;
    let inbound_transport = transport;
    tokio::task::spawn_local(async move {
        let mut rx = inbound_transport.received_data_events();
        loop {
            if let Ok(event) = rx.recv().await {
                let data = event.data.as_slice();
                let destination_hex = hex::encode(event.destination.as_slice());
                if diagnostics_enabled() {
                    eprintln!(
                        "[daemon-rx] dst={} len={} ratchet_used={} data_prefix={}",
                        destination_hex,
                        data.len(),
                        event.ratchet_used,
                        payload_preview(data, 16)
                    );
                } else {
                    eprintln!("[daemon] rx data len={} dst={}", data.len(), destination_hex);
                }
                let mut destination = [0u8; 16];
                destination.copy_from_slice(event.destination.as_slice());
                let payload_mode = inbound_payload_mode(event.payload_mode);
                let record = if diagnostics_enabled() {
                    let (record, diagnostics) =
                        decode_inbound_payload_with_diagnostics(destination, data, payload_mode);
                    if let Some(ref decoded) = record {
                        eprintln!(
                            "[daemon-rx] decoded msg_id={} src={} dst={} title_len={} content_len={}",
                            decoded.id,
                            decoded.source,
                            decoded.destination,
                            decoded.title.len(),
                            decoded.content.len()
                        );
                    } else {
                        eprintln!(
                            "[daemon-rx] decode-failed dst={} attempts={}",
                            destination_hex,
                            diagnostics.summary()
                        );
                    }
                    record
                } else {
                    decode_inbound_payload(destination, data, payload_mode)
                };
                if let Some(record) = record {
                    let _ = daemon_inbound.accept_inbound(record);
                }
            }
        }
    });
}

pub(super) fn take_outbound_resource_message_id(
    outbound_resource_map: &Arc<Mutex<HashMap<String, String>>>,
    resource_hash_hex: &str,
) -> Option<String> {
    outbound_resource_map.lock().ok().and_then(|mut guard| guard.remove(resource_hash_hex))
}

pub(super) fn prune_outbound_resource_mappings_for_message(
    outbound_resource_map: &Arc<Mutex<HashMap<String, String>>>,
    message_id: &str,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.retain(|_, mapped_message_id| mapped_message_id != message_id);
    }
}

async fn resolve_link_destination(
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<[u8; 16]> {
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        let mut destination = [0u8; 16];
        destination.copy_from_slice(guard.destination().address_hash.as_slice());
        return Some(destination);
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        let mut destination = [0u8; 16];
        destination.copy_from_slice(guard.destination().address_hash.as_slice());
        return Some(destination);
    }
    None
}
