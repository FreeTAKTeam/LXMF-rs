use super::*;

#[tokio::test(start_paused = true)]
async fn cached_identity_opportunistic_packet_accepts_real_recipient_proof() {
    prove_owned_opportunistic(false, false).await;
}

#[tokio::test(start_paused = true)]
async fn late_proof_for_first_attempt_completes_retried_owned_delivery() {
    prove_owned_opportunistic(true, false).await;
}

#[tokio::test(start_paused = true)]
async fn retry_status_read_failure_waits_for_recovery_without_competing_terminal() {
    prove_owned_opportunistic(true, true).await;
}

async fn prove_owned_opportunistic(retry_before_proof: bool, read_fault: bool) {
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
    if retry_before_proof {
        sender_channel
            .rx_channel
            .send(RxMessage {
                address: *sender_channel.address(),
                packet: destination.lock().await.announce(OsRng, None).expect("announce"),
                source: IfaceSource::None,
            })
            .await
            .expect("announce ingress");
        while !sender.has_path(&destination_hash).await {
            tokio::task::yield_now().await;
        }
    }
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
        publisher.clone(),
    );
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let (_stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    let packet = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let packet = sender_channel.tx_channel.recv().await.expect("sender channel").packet;
            if packet.destination == destination_hash
                && packet.header.packet_type == PacketType::Data
            {
                break packet;
            }
        }
    })
    .await
    .expect("send timeout");
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(scheduler.owns_delivery(&id), "packet handoff must retain admission until receipt");
    assert!(!owner.is_finished());
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
    if retry_before_proof {
        if read_fault {
            connection.execute_batch("ALTER TABLE messages RENAME COLUMN receipt_status TO unavailable_receipt_status").expect("inject status read failure");
            tokio::time::sleep(Duration::from_secs(12)).await;
            assert!(scheduler.owns_delivery(&id));
            assert!(!owner.is_finished());
            assert!(
                sender_channel.tx_channel.try_recv().is_err(),
                "no retransmission during unknown status"
            );
            assert!(!publisher.terminal_pending(&id).expect("no competing terminal"));
            connection.execute_batch("ALTER TABLE messages RENAME COLUMN unavailable_receipt_status TO receipt_status").expect("recover status reads");
        }
        let second =
            tokio::time::timeout(Duration::from_secs(20), sender_channel.tx_channel.recv())
                .await
                .expect("retry deadline")
                .expect("retry packet")
                .packet;
        assert_eq!(second.destination, destination_hash);
        assert_ne!(second.hash(), packet_hash, "fresh transport encryption on retry");
        assert_eq!(
            receipts.lock().expect("receipt map").len(),
            2,
            "retain earlier proof correlation"
        );
        receiver_channel
            .rx_channel
            .send(RxMessage {
                address: *receiver_channel.address(),
                packet: second,
                source: IfaceSource::None,
            })
            .await
            .expect("second ingress");
        let second_plaintext = received.recv().await.expect("retry plaintext");
        assert_eq!(second_plaintext.data.as_slice(), plaintext.data.as_slice());
        connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE UPDATE OF receipt_status ON messages BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL injected'); END;").expect("receipt fault");
    }
    sender_channel
        .rx_channel
        .send(RxMessage {
            address: *sender_channel.address(),
            packet: proof,
            source: IfaceSource::None,
        })
        .await
        .expect("proof ingress");
    if retry_before_proof {
        while !publisher.terminal_pending(&id).expect("pending") {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(scheduler.owns_delivery(&id));
        assert!(!owner.is_finished(), "accepted proof must remain owned through persistence");
        connection.execute_batch("DROP TRIGGER fail_receipt").expect("recover persistence");
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while daemon.message_receipt_status(&id).expect("status").as_deref() != Some("delivered") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("actual recipient proof must commit delivered");
    tokio::time::timeout(Duration::from_secs(1), owner)
        .await
        .expect("owner released")
        .expect("owner");
    assert!(!scheduler.owns_delivery(&id));
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
    receipt_owner.drain().await.expect("receipt drain");
    assert!(receipts.lock().expect("receipts").is_empty());
    assert_eq!(state(&connection, &id), "terminal");
    assert_eq!(
        sender.has_path(&destination_hash).await,
        retry_before_proof,
        "only the actual announce establishes a route"
    );
    scheduler.shutdown();
}
