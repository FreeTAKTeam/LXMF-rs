use crate::rpc::{RpcDaemon, RpcRequest};
use crate::transport::DeliveryReceipt;
use serde_json::json;
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

pub fn record_receipt_status(
    daemon: &RpcDaemon,
    message_id: &str,
    status: &str,
) -> Result<(), std::io::Error> {
    let _ = daemon.handle_rpc(RpcRequest {
        id: 0,
        method: "record_receipt".into(),
        params: Some(json!({
            "message_id": message_id,
            "status": status,
        })),
    })?;
    Ok(())
}
