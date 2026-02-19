use reticulum::receipt::{
    record_receipt_status, resolve_receipt_message_id,
    track_receipt_mapping as shared_track_receipt_mapping,
};
use reticulum::rpc::RpcDaemon;
use reticulum::transport::{DeliveryReceipt, ReceiptHandler};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
pub struct ReceiptEvent {
    pub message_id: String,
    pub status: String,
}

#[derive(Clone)]
pub struct ReceiptBridge {
    map: Arc<Mutex<HashMap<String, String>>>,
    tx: UnboundedSender<ReceiptEvent>,
}

impl ReceiptBridge {
    pub fn new(
        map: Arc<Mutex<HashMap<String, String>>>,
        tx: UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self { map, tx }
    }
}

impl ReceiptHandler for ReceiptBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let message_id = resolve_receipt_message_id(&self.map, receipt);
        if let Some(message_id) = message_id {
            let _ = self.tx.send(ReceiptEvent { message_id, status: "delivered".into() });
        }
    }
}

pub fn handle_receipt_event(daemon: &RpcDaemon, event: ReceiptEvent) -> Result<(), std::io::Error> {
    record_receipt_status(daemon, &event.message_id, &event.status)
}

pub fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    shared_track_receipt_mapping(map, packet_hash, message_id);
}
