use super::*;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::packet::PacketType;

fn admitted(scheduler: &DeliveryScheduler, mut task: DeliveryTask) -> ScheduledDelivery {
    let reservation =
        scheduler.reserve_message(&task.message_id, 4096).expect("reserve").expect("owner");
    task.wakeup = reservation.ownership.wakeup.clone();
    task.path_requests = scheduler.path_requests.clone();
    scheduler.metrics.record_admitted_for_peer(&task.destination_hex);
    if task.requires_normal_deferred_stamp_work() {
        scheduler.metrics.record_stamp_queued_for_peer(&task.destination_hex);
    }
    ScheduledDelivery {
        task,
        _capacity_permit: reservation.count,
        _bytes: reservation.bytes,
        _ownership: reservation.ownership,
    }
}

#[tokio::test(start_paused = true)]
async fn unresolved_owner_retains_wire_and_capacity_while_another_peer_sends() {
    let mut recipient = SingleInputDestination::new(
        rns_transport::identity::PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination: [u8; 16] = recipient.desc.address_hash.as_slice().try_into().expect("hash");
    let (_directory, connection, mut task) = durable_task_for_destination(destination);
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("storage healthy");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    let transport = task.transport.clone();
    let mut channel = transport.iface_manager().lock().await.new_channel(32);
    assert!(transport.iface_manager().lock().await.set_announce_pacing(
        *channel.address(),
        100,
        100
    ));
    let daemon = task.daemon.clone();
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 2,
        ..Default::default()
    });
    let global = Arc::new(Semaphore::new(1));
    let peer_limit = Arc::new(Semaphore::new(1));
    let stamp = Arc::new(Semaphore::new(1));
    let (_stop, stopping) = tokio::sync::watch::channel(false);
    let first = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        peer_limit.clone(),
        stamp.clone(),
        scheduler.metrics.clone(),
        peer.clone(),
        stopping.clone(),
    ));
    let request = channel.tx_channel.recv().await.expect("discovery request");
    assert_ne!(request.packet.destination, recipient.desc.address_hash);
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    let wire = daemon
        .durable_prepared_payload(&id)
        .expect("lookup")
        .flatten()
        .expect("committed before discovery");
    assert_eq!(state(&connection, &id), "scheduled");
    assert_eq!(global.available_permits(), 1);
    assert_eq!(peer_limit.available_permits(), 1);
    assert_eq!(stamp.available_permits(), 1);
    assert!(scheduler.owns_delivery(&id));
    assert!(scheduler.reserve_message(&id, 4096).expect("duplicate").is_none());
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize - 4096);

    let mut other = delivery_task_for_propagation_cost_lookup(Arc::new(RpcDaemon::test_instance()));
    let remote = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    other.peer_identity = Some(*remote.as_identity());
    other.transport = transport.clone();
    other.message_id = "other-peer-send".into();
    other.requested_method = RequestedDeliveryMethod::Opportunistic;
    other.stamp_cost = Some(0);
    other.destination_hash = remote.as_identity().address_hash;
    other.destination = other.destination_hash.as_slice().try_into().expect("other hash");
    other.destination_hex = hex::encode(other.destination);
    let other_peer = other.destination_hex.clone();
    completion::complete_delivery(
        admitted(&scheduler, other),
        global.clone(),
        Arc::new(Semaphore::new(1)),
        stamp.clone(),
        scheduler.metrics.clone(),
        other_peer,
        stopping,
    )
    .await;
    let sent = channel.tx_channel.recv().await.expect("other peer packet");
    assert_eq!(sent.packet.header.packet_type, PacketType::Data);
    assert!(!first.is_finished());
    assert!(scheduler.owns_delivery(&id));
    assert_eq!(state(&connection, &id), "scheduled", "network miss must not requeue");

    // Make the deadline and a real signed announce ready in the same interval.
    tokio::time::advance(Duration::from_secs(86)).await;
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: recipient.announce(OsRng, None).expect("signed announce"),
            source: IfaceSource::None,
        })
        .await
        .expect("announce ingress");
    while !transport.has_path(&recipient.desc.address_hash).await {
        tokio::task::yield_now().await;
    }
    scheduler.notify_path_available(&peer);
    scheduler.notify_path_available(&peer);
    for _ in 0..32 {
        tokio::task::yield_now().await;
        if first.is_finished() {
            break;
        }
    }
    assert!(first.is_finished(), "announce resumes the same owner without another timer interval");
    first.await.expect("first owner");
    let mut delivered_packets = 0;
    while let Ok(frame) = channel.tx_channel.try_recv() {
        if frame.packet.header.packet_type == PacketType::Data
            && frame.packet.destination == recipient.desc.address_hash
        {
            delivered_packets += 1;
        }
    }
    assert_eq!(delivered_packets, 1, "announce/timer race must not duplicate send");
    assert_eq!(daemon.durable_prepared_payload(&id).expect("lookup").flatten(), Some(wire));
    assert_eq!(scheduler.metrics.snapshot().completed_total, 2);
    assert_eq!(scheduler.metrics.snapshot().queued_current, 0);
    assert_eq!(scheduler.metrics.snapshot().in_flight_current, 0);
    assert!(scheduler.owned_messages.lock().expect("owners").is_empty());
    scheduler.shutdown();
}

#[tokio::test(start_paused = true)]
async fn discovery_shutdown_releases_owner_without_requeue_or_transmission() {
    let (_directory, connection, task) = durable_task();
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    channel.tx_channel.recv().await.expect("request");
    stop.send_replace(true);
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(owner.is_finished());
    owner.await.expect("stopped owner");
    assert_eq!(state(&connection, &id), "scheduled", "startup recovery owns the interrupted claim");
    assert!(scheduler.owned_messages.lock().expect("owners").is_empty());
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
    assert!(channel.tx_channel.try_recv().is_err());
    scheduler.shutdown();
}

#[tokio::test]
async fn shutdown_joins_running_normal_stamp_and_releases_reservations() {
    let (_directory, connection, mut task) = durable_task();
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    task.stamp_cost = Some(255);
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
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
    tokio::time::timeout(Duration::from_secs(5), async {
        while scheduler.metrics.snapshot().stamp_in_flight_current == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stamp began");
    // Let the real blocking PoW worker run; a cost of 255 cannot finish this test.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!owner.is_finished());
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(5), owner)
        .await
        .expect("shutdown cancels and joins PoW")
        .expect("owner");
    assert_eq!(state(&connection, &id), "scheduled");
    assert_eq!(stamp.available_permits(), 1);
    assert_eq!(scheduler.metrics.snapshot().stamp_in_flight_current, 0);
    assert_eq!(scheduler.metrics.snapshot().stamp_queued_current, 0);
    assert_eq!(scheduler.metrics.snapshot().completed_total, 1);
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
    assert!(!scheduler.owns_delivery(&id));
    assert!(channel.tx_channel.try_recv().is_err());
    scheduler.shutdown();
}

#[tokio::test(start_paused = true)]
async fn shutdown_removes_stamp_waiter_without_acquiring_a_permit() {
    let (_directory, connection, task) = durable_task();
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    let peer = task.destination_hex.clone();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let stamp = Arc::new(Semaphore::new(0));
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
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert_eq!(scheduler.metrics.snapshot().stamp_queued_current, 1);
    assert!(!owner.is_finished());
    stop.send_replace(true);
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(owner.is_finished());
    owner.await.expect("stopped stamp waiter");
    assert_eq!(scheduler.metrics.snapshot().stamp_queued_current, 0);
    assert_eq!(scheduler.metrics.snapshot().stamp_in_flight_current, 0);
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(stamp.available_permits(), 0);
    scheduler.shutdown();
}

#[tokio::test(start_paused = true)]
async fn persisted_cancellation_ends_discovery_without_requeue_or_another_request() {
    let (_directory, connection, task) = durable_task();
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    let daemon = task.daemon.clone();
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
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
        peer.clone(),
        stopping,
    ));
    channel.tx_channel.recv().await.expect("initial path request");
    let before = daemon.durable_prepared_payload(&id).expect("prepared");
    daemon.record_network_receipt(&id, "cancelled", serde_json::Map::new()).expect("cancel");
    // An announce racing cancellation must never turn into a fresh attempt.
    scheduler.notify_path_available(&peer);
    tokio::time::advance(Duration::from_millis(250)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(owner.is_finished());
    owner.await.expect("cancelled owner");
    assert_eq!(state(&connection, &id), "terminal");
    assert_eq!(daemon.durable_prepared_payload(&id).expect("prepared"), before);
    assert_eq!(scheduler.metrics.snapshot().queued_current, 0);
    assert_eq!(scheduler.metrics.snapshot().in_flight_current, 0);
    assert_eq!(scheduler.metrics.snapshot().stamp_queued_current, 0);
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert!(!scheduler.owns_delivery(&id));
    assert!(channel.tx_channel.try_recv().is_err());
    scheduler.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_terminal_retains_owner_until_commit_or_shutdown() {
    use reticulum_daemon::receipt_bridge::ReceiptPublisher;
    for shutdown_before_commit in [false, true] {
        let (_directory, connection, mut task) = durable_task();
        connection.execute_batch("DROP TRIGGER fail_prepared; CREATE TRIGGER fail_receipt BEFORE UPDATE OF receipt_status ON messages BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL injected'); END;").expect("receipt fault");
        // Invalid PoW cost fails before any transport handoff and publishes one terminal receipt.
        task.stamp_cost = Some(256);
        let id = task.message_id.clone();
        let peer = task.destination_hex.clone();
        let daemon = task.daemon.clone();
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender);
        publisher.reserve_terminal(&id).expect("terminal capacity");
        task.receipt_tx = publisher.clone();
        let receipt_owner = crate::receipt_worker::spawn_receipt_worker(
            daemon.clone(),
            receiver,
            task.receipt_map.clone(),
            task.outbound_resource_map.clone(),
            publisher.clone(),
        );
        let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
            queue_capacity: 1,
            ..Default::default()
        });
        let (stop, stopping) = tokio::sync::watch::channel(false);
        let owner = tokio::spawn(completion::complete_delivery(
            admitted(&scheduler, task),
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(1)),
            scheduler.metrics.clone(),
            peer,
            stopping,
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while !publisher.terminal_pending(&id).expect("pending lookup") {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("terminal queued");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!owner.is_finished(), "queue acceptance is not durable completion");
        assert!(scheduler.owns_delivery(&id));
        assert!(scheduler.reserve_message(&id, 4096).expect("duplicate").is_none());
        assert_eq!(scheduler.backlog_limit.available_permits(), 0);
        assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize - 4096);
        assert_eq!(state(&connection, &id), "scheduled");
        if shutdown_before_commit {
            stop.send_replace(true);
            tokio::time::timeout(Duration::from_secs(2), owner)
                .await
                .expect("shutdown releases scheduler")
                .expect("owner");
            assert!(publisher.terminal_pending(&id).expect("pending survives shutdown"));
            assert_eq!(
                state(&connection, &id),
                "scheduled",
                "shutdown must not requeue an uncommitted terminal"
            );
            connection.execute_batch("DROP TRIGGER fail_receipt;").expect("recover storage");
        } else {
            connection.execute_batch("DROP TRIGGER fail_receipt;").expect("recover storage");
            tokio::time::timeout(Duration::from_secs(3), owner)
                .await
                .expect("commit releases owner")
                .expect("owner");
            assert!(!publisher.terminal_pending(&id).expect("committed"));
        }
        tokio::time::timeout(Duration::from_secs(3), receipt_owner.drain())
            .await
            .expect("drain deadline")
            .expect("receipt drained");
        assert_eq!(state(&connection, &id), "terminal");
        assert!(!scheduler.owns_delivery(&id));
        assert_eq!(scheduler.backlog_limit.available_permits(), 1);
        assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
        assert!(!publisher.terminal_pending(&id).expect("committed"));
        let receipts: i64 = connection
            .query_row("SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'", [], |row| {
                row.get(0)
            })
            .expect("receipt journal");
        assert_eq!(receipts, 1);
        scheduler.shutdown();
    }
}
