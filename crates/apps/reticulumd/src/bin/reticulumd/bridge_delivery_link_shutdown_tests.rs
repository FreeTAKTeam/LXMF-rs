use super::*;
use rand_core::RngCore;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn coordinated_shutdown_interrupts_another_native_operations_blocked_handler() {
    let signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let recipient =
        SingleInputDestination::new(signer.clone(), DestinationName::new("lxmf", "delivery"));
    let mut bytes = vec![0; 8192];
    OsRng.fill_bytes(&mut bytes);
    let content = hex::encode(bytes);
    let (_directory, connection, mut task) = durable_task_for_content(
        recipient.desc.address_hash.as_slice().try_into().expect("recipient"),
        &content,
    );
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    task.peer_identity = Some(*signer.as_identity());
    let (link, mut channel, peer_link) = active_owner_link(&task, &signer, recipient.desc).await;
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let transport = task.transport.clone();
    let resources = task.outbound_resource_map.clone();
    let (receipt_sender, receipt_receiver) = tokio::sync::mpsc::channel(4);
    task.receipt_tx = receipt_sender.into();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig::default());
    let global = Arc::new(Semaphore::new(1));
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    let identify = channel.tx_channel.recv().await.expect("identify");
    assert_eq!(identify.packet.context, PacketContext::LinkIdentify);
    let advertisement = channel.tx_channel.recv().await.expect("advertisement").packet;
    assert_eq!(advertisement.context, PacketContext::ResourceAdvrtisement);
    let mut plain = vec![0; 500];
    let advertisement = rns_transport::resource::ResourceAdvertisement::unpack(
        peer_link.decrypt(advertisement.data.as_slice(), &mut plain).expect("peer decrypts"),
    )
    .expect("native advertisement");
    let hashes = advertisement.hashmap.as_chunks::<4>().0.to_vec();
    assert!(hashes.len() > 1, "several Resource responses must fill the queue");
    let filler =
        link.lock().await.data_packet(b"queue occupancy").expect("unrelated native packet");
    assert_eq!(
        transport.send_link_packet_on_bound_iface(&link, filler).await,
        rns_transport::transport::SendPacketOutcome::SentDirect
    );
    let request = rns_transport::resource::ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash: advertisement.hash,
        requested_hashes: hashes,
    };
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: rns_transport::resource::build_resource_request_packet(&peer_link, &request),
            source: IfaceSource::None,
        })
        .await
        .expect("native RX Resource request");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if tokio::time::timeout(
                Duration::from_millis(100),
                transport.path_timing(&recipient.desc.address_hash),
            )
            .await
            .is_err()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the separate RX handler is blocked sending Resource responses");
    assert!(scheduler.owns_delivery(&id));
    // Let the owner's ordinary 250 ms observation turn encounter that held
    // handler before requesting shutdown; cancellation alone is insufficient.
    tokio::time::sleep(Duration::from_millis(350)).await;
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .expect("bounded coordinated shutdown")
        .expect("owner");
    assert!(!scheduler.owns_delivery(&id));
    assert_eq!(global.available_permits(), 1);
    assert_eq!(
        resources.lock().expect("correlation").get(&advertisement.hash.to_string()),
        Some(&id),
        "unresolved native work cannot be falsely retired at shutdown"
    );
    assert_eq!(state(&connection, &id), "scheduled");
    // Daemon shutdown owns the remaining native transport lifetime. Release the
    // independent RX operation before explicitly closing that transport state.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                _ = transport.path_timing(&recipient.desc.address_hash) => break,
                frame = channel.tx_channel.recv() => { frame.expect("blocked native responses"); },
            }
        }
    })
    .await
    .expect("RX handler released");
    let close = transport.teardown_link(&link);
    tokio::pin!(close);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                result = &mut close => { result.expect("native shutdown cleanup"); break; },
                frame = channel.tx_channel.recv() => { frame.expect("native close output"); },
            }
        }
    })
    .await
    .expect("native cleanup deadline");
    drop(receipt_receiver);
    scheduler.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_stamp_status_read_failures_preserve_the_build_retry_budget() {
    let (_directory, connection, mut task) = durable_task();
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy preparation storage");
    task.stamp_cost = Some(255);
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let daemon = task.daemon.clone();
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    let (sender, mut receipts) = tokio::sync::mpsc::channel(4);
    task.receipt_tx = sender.into();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig::default());
    let stamp = Arc::new(Semaphore::new(1));
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        stamp.clone(),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    for _ in 0..3 {
        wait_stamp_workers(&scheduler.metrics, 1).await;
        connection
            .execute_batch(
                "ALTER TABLE messages RENAME COLUMN receipt_status TO unavailable_receipt_status",
            )
            .expect("interrupt only receipt observation");
        wait_stamp_workers(&scheduler.metrics, 0).await;
        assert!(scheduler.owns_delivery(&id));
        assert!(!owner.is_finished());
        assert_eq!(
            stamp.available_permits(),
            1,
            "interrupted work joins before waiting for storage"
        );
        connection
            .execute_batch(
                "ALTER TABLE messages RENAME COLUMN unavailable_receipt_status TO receipt_status",
            )
            .expect("restore receipt reads");
        wait_stamp_workers(&scheduler.metrics, 1).await;
        assert_eq!(
            scheduler.metrics.snapshot().stamp_retried_total,
            0,
            "observation failure is not a failed payload-build attempt"
        );
    }
    let result = daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_messages".into(), params: None })
        .expect("message metadata")
        .result
        .expect("messages");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some(id.as_str()))
        .expect("admitted message");
    assert_eq!(message["fields"]["_lxmf"]["stamp_attempts"], json!(1));
    assert!(receipts.try_recv().is_err(), "no competing terminal publication");
    assert!(channel.tx_channel.try_recv().is_err(), "no unprepared network send");
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(3), owner)
        .await
        .expect("joined stamp shutdown")
        .expect("owner");
    assert_eq!(state(&connection, &id), "scheduled");
    assert_eq!(stamp.available_permits(), 1);
    assert!(!scheduler.owns_delivery(&id));
    scheduler.shutdown();
}

async fn wait_stamp_workers(metrics: &DeliverySchedulerMetrics, expected: u64) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while metrics.snapshot().stamp_in_flight_current != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stamp worker transition");
}
