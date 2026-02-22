use super::{now_epoch_secs, parse_alternative_relay_request_status};
use reticulum::hash::AddressHash;
use reticulum::receipt::{
    record_receipt_status as shared_record_receipt_status,
    resolve_receipt_message_id as shared_resolve_receipt_message_id,
};
use reticulum::rpc::{RpcDaemon, RpcEvent};
use reticulum::transport::{DeliveryReceipt, ReceiptHandler, Transport};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub(super) struct ReceiptEvent {
    pub(super) message_id: String,
    pub(super) status: String,
}

#[derive(Clone)]
pub(super) struct ReceiptBridge {
    map: Arc<Mutex<HashMap<String, String>>>,
    delivered_messages: Arc<Mutex<HashSet<String>>>,
    tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

impl ReceiptBridge {
    pub(super) fn new(
        map: Arc<Mutex<HashMap<String, String>>>,
        delivered_messages: Arc<Mutex<HashSet<String>>>,
        tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self { map, delivered_messages, tx }
    }
}

impl ReceiptHandler for ReceiptBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let message_id = shared_resolve_receipt_message_id(&self.map, receipt);
        if let Some(message_id) = message_id {
            if let Ok(mut delivered) = self.delivered_messages.lock() {
                delivered.insert(message_id.clone());
            }
            let _ = self.tx.send(ReceiptEvent { message_id, status: "delivered".into() });
        }
    }
}

pub(super) fn handle_receipt_event(
    daemon: &RpcDaemon,
    event: ReceiptEvent,
) -> Result<(), std::io::Error> {
    let message_id = event.message_id;
    let status = event.status;
    shared_record_receipt_status(daemon, &message_id, &status)?;
    if let Some(exclude_relays) = parse_alternative_relay_request_status(status.as_str()) {
        daemon.push_event(RpcEvent {
            event_type: "alternative_relay_request".to_string(),
            payload: json!({
                "message_id": message_id,
                "exclude_relays": exclude_relays,
                "timestamp_ms": (now_epoch_secs() as i64) * 1000,
            }),
        });
    }
    Ok(())
}

pub(super) async fn resolve_link_destination(
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
