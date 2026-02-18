use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use reticulum::packet::{Packet, PacketContext, PacketDataBuffer, PacketType};
use reticulum::transport::{DeliveryReceipt, ReceiptHandler, Transport, TransportConfig};

struct Counter {
    count: Arc<AtomicUsize>,
}

impl ReceiptHandler for Counter {
    fn on_receipt(&self, _receipt: &DeliveryReceipt) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

struct ReceiptCapture {
    receipt: Arc<std::sync::Mutex<Option<[u8; 32]>>>,
}

impl ReceiptHandler for ReceiptCapture {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let mut guard = self.receipt.lock().unwrap();
        *guard = Some(receipt.message_id);
    }
}

#[tokio::test]
async fn proof_packet_emits_receipt() {
    let count = Arc::new(AtomicUsize::new(0));
    let handler = Counter { count: Arc::clone(&count) };

    let mut transport = Transport::new(TransportConfig::default());
    transport.set_receipt_handler(Box::new(handler)).await;

    let mut packet = Packet::default();
    packet.header.packet_type = PacketType::Proof;
    packet.context = PacketContext::None;
    packet.data = PacketDataBuffer::new_from_slice(&[1u8; 32]);

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn proof_packet_uses_payload_hash() {
    let receipt = Arc::new(std::sync::Mutex::new(None));
    let handler = ReceiptCapture { receipt: Arc::clone(&receipt) };

    let mut transport = Transport::new(TransportConfig::default());
    transport.set_receipt_handler(Box::new(handler)).await;

    let expected = [9u8; 32];
    let mut packet = Packet::default();
    packet.header.packet_type = PacketType::Proof;
    packet.context = PacketContext::None;
    packet.data = PacketDataBuffer::new_from_slice(&expected);

    transport.handle_inbound_for_test(packet).await;

    let captured = receipt.lock().unwrap().expect("receipt");
    assert_eq!(captured, expected);
}
