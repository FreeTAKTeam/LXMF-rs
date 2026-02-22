use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_transport::transport::{DeliveryReceipt, ReceiptHandler};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn receipt_bridge_emits_event_for_known_packet() {
    let (tx, mut rx) = unbounded_channel();
    let map = Arc::new(Mutex::new(HashMap::new()));
    let packet_id = [7u8; 32];
    let packet_hex = hex::encode(packet_id);
    map.lock().unwrap().insert(packet_hex, "msg-1".to_string());

    let bridge = ReceiptBridge::new(map.clone(), tx);
    bridge.on_receipt(&DeliveryReceipt::new(packet_id));

    let event = rx.recv().await.expect("receipt event");
    assert_eq!(event.message_id, "msg-1");
    assert_eq!(event.status, "delivered");
}
