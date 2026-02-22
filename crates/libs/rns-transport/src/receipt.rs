use crate::transport::DeliveryReceipt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub fn resolve_receipt_message_id(
    map: &Arc<Mutex<HashMap<String, String>>>,
    receipt: &DeliveryReceipt,
) -> Option<String> {
    let key = hex::encode(receipt.message_id);
    map.lock().ok().and_then(|mut guard| guard.remove(&key))
}

pub fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    if let Ok(mut guard) = map.lock() {
        guard.insert(packet_hash.to_string(), message_id.to_string());
    }
}

pub fn prune_receipt_mappings_for_message(
    map: &Arc<Mutex<HashMap<String, String>>>,
    message_id: &str,
) {
    if let Ok(mut guard) = map.lock() {
        guard.retain(|_, mapped_message_id| mapped_message_id != message_id);
    }
}

pub trait ReceiptRecordSink {
    fn record_receipt_status(&self, message_id: &str, status: &str) -> std::io::Result<()>;
}

impl<F> ReceiptRecordSink for F
where
    F: Fn(&str, &str) -> std::io::Result<()>,
{
    fn record_receipt_status(&self, message_id: &str, status: &str) -> std::io::Result<()> {
        self(message_id, status)
    }
}

pub fn record_receipt_status(
    sink: &impl ReceiptRecordSink,
    message_id: &str,
    status: &str,
) -> Result<(), std::io::Error> {
    sink.record_receipt_status(message_id, status)
}
