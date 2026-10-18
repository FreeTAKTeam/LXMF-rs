use super::*;

#[tokio::test]
async fn cached_identity_opportunistic_packet_accepts_real_recipient_proof() {
    use reticulum_daemon::receipt_bridge::{ReceiptBridge, ReceiptPublisher};
    use rns_transport::destination::ProofStrategy;
    use rns_transport::transport::{Transport, TransportConfig};
    let remote = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let receiver = Transport::new(TransportConfig::new("opportunistic-recipient", &remote, true));
    let destination =
        receiver.add_destination(remote.clone(), DestinationName::new("lxmf", "delivery")).await;
    destination.lock().await.set_proof_strategy(ProofStrategy::All);
    let destination_hash = destination.lock().await.desc.address_hash;
    let mut receiver_channel = receiver.iface_manager().lock().await.new_channel(16);
    let mut received = receiver.received_data_events();
    let (_directory, connection, mut task) =
        durable_task_for_destination(destination_hash.as_slice().try_into().expect("hash"));
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    task.peer_identity = Some(*remote.as_identity());
    let sender = task.transport.clone();
    assert!(!sender.has_path(&destination_hash).await);
    assert!(
        sender.destination_identity(&destination_hash).await.is_none(),
        "identity starts only in the daemon cache"
    );
    let mut sender_channel = sender.iface_manager().lock().await.new_channel(16);
    let daemon = task.daemon.clone();
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let receipts = task.receipt_map.clone();
    let (receipt_tx, receipt_rx) = tokio::sync::mpsc::channel(8);
    let publisher = ReceiptPublisher::from(receipt_tx);
    publisher.reserve_terminal(&id).expect("terminal capacity");
    task.receipt_tx = publisher.clone();
    sender
        .set_receipt_handler(Box::new(ReceiptBridge::new(receipts.clone(), publisher.clone())))
        .await;
    let receipt_owner = crate::receipt_worker::spawn_receipt_worker(
        daemon.clone(),
        receipt_rx,
        receipts.clone(),
        task.outbound_resource_map.clone(),
        publisher,
    );
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let (_stop, stopping) = tokio::sync::watch::channel(false);
    completion::complete_delivery(
        admitted(&scheduler, task),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    )
    .await;
    let packet = tokio::time::timeout(Duration::from_secs(1), sender_channel.tx_channel.recv())
        .await
        .expect("send timeout")
        .expect("sent packet")
        .packet;
    let packet_hash = packet.hash();
    receiver_channel
        .rx_channel
        .send(RxMessage { address: *receiver_channel.address(), packet, source: IfaceSource::None })
        .await
        .expect("recipient ingress");
    let plaintext = tokio::time::timeout(Duration::from_secs(1), received.recv())
        .await
        .expect("receive timeout")
        .expect("plaintext");
    let prepared = decode_prepared(
        daemon.durable_prepared_payload(&id).expect("lookup").flatten().expect("prepared"),
    )
    .expect("decode");
    let mut wire = destination_hash.as_slice().to_vec();
    wire.extend_from_slice(plaintext.data.as_slice());
    assert_eq!(
        wire, prepared.lxmf_payload,
        "exact signed LXMF bytes after one transport decryption"
    );
    let proof = tokio::time::timeout(Duration::from_secs(1), receiver_channel.tx_channel.recv())
        .await
        .expect("proof timeout")
        .expect("proof")
        .packet;
    assert_eq!(proof.header.packet_type, PacketType::Proof);
    assert_eq!(proof.destination, rns_transport::hash::AddressHash::new_from_hash(&packet_hash));
    sender_channel
        .rx_channel
        .send(RxMessage {
            address: *sender_channel.address(),
            packet: proof,
            source: IfaceSource::None,
        })
        .await
        .expect("proof ingress");
    tokio::time::timeout(Duration::from_secs(1), async {
        while daemon.message_receipt_status(&id).expect("status").as_deref() != Some("delivered") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("actual recipient proof must commit delivered");
    receipt_owner.drain().await.expect("receipt drain");
    assert!(receipts.lock().expect("receipts").is_empty());
    assert_eq!(state(&connection, &id), "terminal");
    assert!(
        !sender.has_path(&destination_hash).await,
        "proof identity registration must not invent a route"
    );
    scheduler.shutdown();
}
