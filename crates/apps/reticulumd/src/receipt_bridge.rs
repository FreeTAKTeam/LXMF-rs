use rns_rpc::RpcDaemon;
use rns_transport::receipt::{
    lookup_receipt_message_id, record_receipt_status,
    track_receipt_mapping as shared_track_receipt_mapping,
};
use rns_transport::transport::{DeliveryReceipt, ReceiptHandler};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub struct ReceiptEvent {
    pub message_id: String,
    pub status: String,
}

#[derive(Clone)]
pub struct ReceiptBridge {
    map: Arc<Mutex<HashMap<String, String>>>,
    tx: Sender<ReceiptEvent>,
}

impl ReceiptBridge {
    pub fn new(map: Arc<Mutex<HashMap<String, String>>>, tx: Sender<ReceiptEvent>) -> Self {
        Self { map, tx }
    }
}

impl ReceiptHandler for ReceiptBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let message_id = match lookup_receipt_message_id(&self.map, receipt) {
            Ok(id) => id,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                log::warn!("[daemon] receipt map error: {err}");
                return;
            }
        };
        if let Err(err) = self.tx.try_send(ReceiptEvent { message_id, status: "delivered".into() })
        {
            log::warn!("[daemon] dropped delivery receipt event: {err}");
        }
    }
}

pub fn handle_receipt_event(daemon: &RpcDaemon, event: ReceiptEvent) -> Result<(), std::io::Error> {
    if event.status.eq_ignore_ascii_case("delivered") {
        daemon.record_message_delivery_receipt(event.message_id.as_str())?;
    }
    record_receipt_status(
        &|message_id: &str, status: &str| {
            let _ = daemon.handle_rpc(rns_rpc::rpc::RpcRequest {
                id: 0,
                method: "record_receipt".into(),
                params: Some(serde_json::json!({
                    "message_id": message_id,
                    "status": status,
                })),
            })?;
            Ok(())
        },
        &event.message_id,
        &event.status,
    )
}

pub fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    shared_track_receipt_mapping(map, packet_hash, message_id);
}
