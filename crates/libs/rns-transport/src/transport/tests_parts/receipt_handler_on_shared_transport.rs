/// The receipt handler lives behind the transport's own lock, so installing
/// it needs no exclusive access. Through `&mut self` a `Transport` already
/// shared as an `Arc` — how every client holds one — could never receive
/// delivery proofs at all.
#[tokio::test]
async fn a_receipt_handler_can_be_installed_on_a_shared_transport() {
    struct Recording(std::sync::Mutex<Vec<[u8; 32]>>);
    impl ReceiptHandler for Recording {
        fn on_receipt(&self, receipt: &DeliveryReceipt) {
            self.0.lock().expect("recording lock").push(receipt.packet_hash().to_bytes());
        }
    }

    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Arc::new(Transport::new(TransportConfig::new("shared", &local_identity, true)));
    let seen = Arc::new(Recording(std::sync::Mutex::new(Vec::new())));

    transport.set_receipt_handler(Box::new(SharedRecording(seen.clone()))).await;
    transport.emit_receipt_for_test(DeliveryReceipt::new([7u8; 32]));

    assert_eq!(seen.0.lock().expect("recording lock").as_slice(), &[[7u8; 32]]);

    struct SharedRecording(Arc<Recording>);
    impl ReceiptHandler for SharedRecording {
        fn on_receipt(&self, receipt: &DeliveryReceipt) {
            self.0.on_receipt(receipt);
        }
    }
}
