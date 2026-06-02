use reticulum_daemon::receipt_bridge::ReceiptBridge;
use rns_transport::transport::{DeliveryReceipt, ReceiptHandler};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{channel, error::TryRecvError};

#[tokio::test]
async fn receipt_bridge_emits_event_for_known_packet() {
    let (tx, mut rx) = channel(4);
    let map = Arc::new(Mutex::new(HashMap::new()));
    let packet_id = [7u8; 32];
    let packet_hex = hex::encode(packet_id);
    map.lock().unwrap().insert(packet_hex.clone(), "msg-1".to_string());

    let bridge = ReceiptBridge::new(map.clone(), tx);
    bridge.on_receipt(&DeliveryReceipt::new(packet_id));

    let event = rx.recv().await.expect("receipt event");
    assert_eq!(event.message_id, "msg-1");
    assert_eq!(event.status, "delivered");
    assert_eq!(
        map.lock().unwrap().get(&packet_hex).map(String::as_str),
        Some("msg-1"),
        "transport receipts should not consume the correlation mapping before terminal receipt persistence",
    );
}

#[tokio::test]
async fn receipt_bridge_drops_when_bounded_queue_is_full() {
    let (tx, mut rx) = channel(1);
    let map = Arc::new(Mutex::new(HashMap::new()));
    let first_packet_id = [7u8; 32];
    let second_packet_id = [8u8; 32];
    map.lock().unwrap().insert(hex::encode(first_packet_id), "msg-1".to_string());
    map.lock().unwrap().insert(hex::encode(second_packet_id), "msg-2".to_string());

    let bridge = ReceiptBridge::new(map, tx);
    bridge.on_receipt(&DeliveryReceipt::new(first_packet_id));
    bridge.on_receipt(&DeliveryReceipt::new(second_packet_id));

    let event = rx.try_recv().expect("first receipt event should fit");
    assert_eq!(event.message_id, "msg-1");
    assert_eq!(rx.try_recv().expect_err("second receipt should be dropped"), TryRecvError::Empty);
}
