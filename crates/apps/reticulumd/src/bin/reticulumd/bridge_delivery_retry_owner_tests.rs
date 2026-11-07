use super::*;

#[tokio::test(start_paused = true)]
async fn no_route_remains_owned_until_reference_retry_exhaustion() {
    use reticulum_daemon::receipt_bridge::ReceiptPublisher;
    let recipient = SingleInputDestination::new(
        rns_transport::identity::PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination = recipient.desc.address_hash;
    let (_directory, connection, mut task) =
        durable_task_for_destination(destination.as_slice().try_into().expect("hash"));
    connection.execute_batch("DROP TRIGGER fail_prepared").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    task.peer_identity = Some(*recipient.identity.as_identity());
    let daemon = task.daemon.clone();
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let receipts = task.receipt_map.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_terminal(&id).expect("reserve");
    task.receipt_tx = publisher.clone();
    let worker = crate::receipt_worker::spawn_receipt_worker(
        daemon.clone(),
        receiver,
        receipts.clone(),
        publisher,
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
    tokio::time::timeout(Duration::from_secs(20), async {
        while daemon.message_receipt_status(&id).expect("status").as_deref()
            != Some("pending: opportunistic no route")
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("no-route dispatch remains nonterminal");
    assert!(scheduler.owns_delivery(&id));
    assert!(!owner.is_finished());
    assert_eq!(state(&connection, &id), "scheduled");
    assert!(
        receipts.lock().expect("receipts").is_empty(),
        "no-route removes only unsent packet correlation"
    );
    let prepared = daemon.durable_prepared_payload(&id).expect("prepared lookup");
    tokio::time::timeout(Duration::from_secs(90), owner)
        .await
        .expect("bounded exhaustion")
        .expect("owner");
    worker.drain().await.expect("drain");
    assert!(daemon
        .message_receipt_status(&id)
        .expect("status")
        .expect("receipt")
        .contains("opportunistic delivery exhausted"));
    assert_eq!(daemon.durable_prepared_payload(&id).expect("prepared lookup"), prepared);
    assert_eq!(state(&connection, &id), "terminal");
    assert!(!scheduler.owns_delivery(&id));
    assert_eq!(scheduler.metrics.snapshot().completed_total, 1);
    scheduler.shutdown();
}

#[tokio::test(start_paused = true)]
async fn early_announce_resumes_during_owned_rediscovery_delay_and_shutdown_cancels_request() {
    rediscovery_deadline(false).await;
}

#[tokio::test(start_paused = true)]
async fn forced_request_is_not_blocked_by_delivery_execution_capacity() {
    rediscovery_deadline(true).await;
}

async fn rediscovery_deadline(block_execution: bool) {
    let mut recipient = SingleInputDestination::new(
        rns_transport::identity::PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination = recipient.desc.address_hash;
    let (_directory, connection, mut task) =
        durable_task_for_destination(destination.as_slice().try_into().expect("hash"));
    connection.execute_batch("DROP TRIGGER fail_prepared").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    let transport = task.transport.clone();
    let mut channel = transport.iface_manager().lock().await.new_channel(32);
    let slow_channel = transport.iface_manager().lock().await.new_channel(32);
    assert!(transport.iface_manager().lock().await.set_announce_pacing(
        *channel.address(),
        100_000,
        100_000
    ));
    assert!(transport.iface_manager().lock().await.set_announce_pacing(
        *slow_channel.address(),
        100,
        100
    ));
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: recipient.announce(OsRng, None).expect("announce"),
            source: IfaceSource::None,
        })
        .await
        .expect("announce ingress");
    while !transport.has_path(&destination).await {
        tokio::task::yield_now().await;
    }
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let global = Arc::new(Semaphore::new(1));
    let peer_limit = Arc::new(Semaphore::new(1));
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        peer_limit.clone(),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer.clone(),
        stopping,
    ));
    for _ in 0..2 {
        let sent = tokio::time::timeout(Duration::from_secs(12), channel.tx_channel.recv())
            .await
            .expect("send deadline")
            .expect("send");
        assert_eq!(sent.packet.destination, destination);
    }
    tokio::time::advance(Duration::from_secs(10) + Duration::from_millis(1)).await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(!transport.has_path(&destination).await, "two known-path attempts force expiry");
    assert!(scheduler.owns_delivery(&id));
    assert_eq!(global.available_permits(), 1);
    assert_eq!(peer_limit.available_permits(), 1);
    tokio::time::advance(Duration::from_millis(100)).await;
    let blocked = if block_execution {
        Some(global.clone().acquire_owned().await.expect("block execution lane"))
    } else {
        None
    };
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: recipient.announce(OsRng, None).expect("new announce"),
            source: IfaceSource::None,
        })
        .await
        .expect("announce ingress");
    while !transport.has_path(&destination).await {
        tokio::task::yield_now().await;
    }
    scheduler.notify_path_available(&peer);
    scheduler.notify_path_available(&peer);
    if block_execution {
        let request = tokio::time::timeout(Duration::from_millis(500), channel.tx_channel.recv())
            .await
            .expect("forced request does not wait for execution capacity")
            .expect("request");
        assert_ne!(request.packet.destination, destination);
        assert!(scheduler.owns_delivery(&id));
        drop(blocked);
    }
    let sent = tokio::time::timeout(Duration::from_millis(100), channel.tx_channel.recv())
        .await
        .expect("announce resumes before 500ms delay")
        .expect("third send");
    assert_eq!(sent.packet.destination, destination);
    stop.send_replace(true);
    owner.await.expect("shutdown joins owner");
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    assert!(
        channel.tx_channel.try_recv().is_err(),
        "no detached rediscovery request or duplicate send"
    );
    assert!(!scheduler.owns_delivery(&id));
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(state(&connection, &id), "scheduled", "shutdown leaves recovery ownership durable");
    scheduler.shutdown();
}

#[tokio::test(start_paused = true)]
async fn terminal_received_while_execution_capacity_is_blocked_prevents_transmission() {
    use reticulum_daemon::receipt_bridge::{ReceiptEvent, ReceiptPublisher};
    let mut recipient = SingleInputDestination::new(
        rns_transport::identity::PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination = recipient.desc.address_hash;
    let (_directory, connection, mut task) =
        durable_task_for_destination(destination.as_slice().try_into().expect("hash"));
    connection.execute_batch("DROP TRIGGER fail_prepared").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    let transport = task.transport.clone();
    let mut channel = transport.iface_manager().lock().await.new_channel(16);
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: recipient.announce(OsRng, None).expect("announce"),
            source: IfaceSource::None,
        })
        .await
        .expect("ingress");
    while !transport.has_path(&destination).await {
        tokio::task::yield_now().await;
    }
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_terminal(&id).expect("terminal reservation");
    task.receipt_tx = publisher.clone();
    let worker = crate::receipt_worker::spawn_receipt_worker(
        task.daemon.clone(),
        receiver,
        task.receipt_map.clone(),
        publisher.clone(),
    );
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let global = Arc::new(Semaphore::new(0));
    let (_stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(scheduler.owns_delivery(&id));
    publisher.try_send(ReceiptEvent::new(&id, "cancelled")).expect("accepted cancellation");
    tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .expect("cancelled without acquiring execution capacity")
        .expect("owner");
    global.add_permits(1);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(channel.tx_channel.try_recv().is_err());
    worker.drain().await.expect("drain");
    assert_eq!(state(&connection, &id), "terminal");
    assert!(!scheduler.owns_delivery(&id));
    scheduler.shutdown();
}

#[tokio::test(start_paused = true)]
async fn oversized_opportunistic_status_failure_retains_owner_until_recovery() {
    let signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let recipient =
        SingleInputDestination::new(signer.clone(), DestinationName::new("lxmf", "delivery"));
    let content = "large payload ".repeat(100);
    let (_directory, connection, mut task) = durable_task_for_content(
        recipient.desc.address_hash.as_slice().try_into().expect("destination"),
        &content,
    );
    connection.execute_batch("DROP TRIGGER fail_prepared").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    task.peer_identity = Some(*signer.as_identity());
    let (_link, mut channel, _peer_link) =
        super::retained_link_tests::active_owner_link(&task, &signer, recipient.desc).await;
    let daemon = task.daemon.clone();
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let publisher = task.receipt_tx.clone();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let global = Arc::new(Semaphore::new(0));
    let peer_limit = Arc::new(Semaphore::new(1));
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        peer_limit.clone(),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while peer_limit.available_permits() != 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("owner waiting for execution capacity");
    let prepared =
        daemon.durable_prepared_payload(&id).expect("prepared").flatten().expect("committed");
    assert!(
        hex::decode(prepared["lxmf_hex"].as_str().expect("wire hex")).expect("wire").len()
            > lxmf::constants::ENCRYPTED_PACKET_MDU + 16
    );
    connection
        .execute_batch(
            "ALTER TABLE messages RENAME COLUMN receipt_status TO unavailable_receipt_status",
        )
        .expect("inject read failure");
    global.add_permits(1);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!owner.is_finished(), "AwaitStatus retains oversized fallback admission");
    assert!(scheduler.owns_delivery(&id));
    assert_eq!(state(&connection, &id), "scheduled");
    assert!(!publisher.terminal_pending(&id).expect("no manufactured terminal"));
    assert_eq!(global.available_permits(), 1);
    assert_eq!(peer_limit.available_permits(), 1);
    connection
        .execute_batch(
            "ALTER TABLE messages RENAME COLUMN unavailable_receipt_status TO receipt_status",
        )
        .expect("recover reads");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(scheduler.owns_delivery(&id), "recovery retains the active native transfer");
    assert_eq!(daemon.durable_prepared_payload(&id).expect("prepared").flatten(), Some(prepared));
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(1), owner).await.expect("shutdown").expect("owner");
    assert!(!scheduler.owns_delivery(&id));
    while channel.tx_channel.try_recv().is_ok() {}
    scheduler.shutdown();
}

#[path = "bridge_delivery_retry_timing_tests.rs"]
mod timing_tests;
