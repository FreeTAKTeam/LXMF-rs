use super::*;
use reticulum_daemon::receipt_bridge::ReceiptPublisher;

#[tokio::test]
async fn shutdown_after_local_ingestion_still_hands_reserved_custody_to_worker() {
    let (_directory, connection, mut task, mut prepared, _old_receipts) =
        super::super::attempt_tests::fallback_task().await;
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Propagated;
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_completion(&task.message_id, true).expect("paired capacity");
    task.receipt_tx = publisher.clone();
    prepare_propagation(
        &task,
        &mut prepared,
        &Arc::new(Semaphore::new(1)),
        &Arc::new(DeliverySchedulerMetrics::default()),
        &task.destination_hex,
    )
    .await
    .expect("prepare local envelope");
    let crate::bridge::delivery_attempt::DeliveryAttemptOutcome::PropagationCustody(event) =
        task.run_prepared_attempt(&mut prepared).await
    else {
        panic!("local custody expected");
    };
    publisher.stop_admission();
    let (_stop, mut stopping) = tokio::sync::watch::channel(true);
    completion::await_custody_commit(&task, *event, &mut stopping).await;
    let owner = crate::receipt_worker::spawn_receipt_worker(
        task.daemon.clone(),
        receiver,
        task.receipt_map.clone(),
        task.outbound_resource_map.clone(),
        publisher,
    );
    tokio::time::timeout(Duration::from_secs(3), owner.drain())
        .await
        .expect("drain deadline")
        .expect("custody drained");
    assert_eq!(state(&connection, &task.message_id), "custody");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fatal_custody_failure_retains_delivery_admission_until_coordinated_shutdown() {
    let (_directory, connection, mut task, _prepared, _old_receipts) =
        super::super::attempt_tests::fallback_task().await;
    connection.execute_batch("DROP TRIGGER fail_prepared; CREATE TRIGGER fail_custody BEFORE UPDATE OF receipt_status ON messages WHEN NEW.receipt_status LIKE 'sent:%' BEGIN SELECT RAISE(ABORT,'invalid custody storage'); END;").expect("fatal custody fault");
    task.requested_method = RequestedDeliveryMethod::Propagated;
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_completion(&id, true).expect("paired capacity");
    task.receipt_tx = publisher.clone();
    let mut receipt_owner = crate::receipt_worker::spawn_receipt_worker(
        task.daemon.clone(),
        receiver,
        task.receipt_map.clone(),
        task.outbound_resource_map.clone(),
        publisher,
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
    let failure = tokio::time::timeout(Duration::from_secs(3), receipt_owner.wait_failure())
        .await
        .expect("worker failure");
    assert!(failure.contains("invalid custody storage"));
    assert!(!owner.is_finished(), "failed observation cannot release admission as success");
    assert!(scheduler.owns_delivery(&id));
    assert_eq!(scheduler.backlog_limit.available_permits(), 0);
    assert_eq!(state(&connection, &id), "scheduled");
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .expect("shutdown deadline")
        .expect("owner");
    assert!(receipt_owner.drain().await.is_err());
    assert!(!scheduler.owns_delivery(&id));
    assert_eq!(
        state(&connection, &id),
        "scheduled",
        "failed custody remains restart-recoverable without deferred requeue"
    );
    scheduler.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_custody_retains_admission_but_releases_execution_until_commit_or_shutdown() {
    for shutdown_before_commit in [false, true] {
        let (_directory, connection, mut task, _prepared, _old_receipts) =
            super::super::attempt_tests::fallback_task().await;
        connection.execute_batch("DROP TRIGGER fail_prepared; CREATE TRIGGER fail_custody BEFORE UPDATE OF receipt_status ON messages WHEN NEW.receipt_status LIKE 'sent:%' BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL injected'); END;").expect("custody fault");
        task.requested_method = RequestedDeliveryMethod::Propagated;
        let id = task.message_id.clone();
        let peer = task.destination_hex.clone();
        let daemon = task.daemon.clone();
        let transport = task.transport.clone();
        let mut channel = transport.iface_manager().lock().await.new_channel(32);
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender.clone());
        publisher.reserve_completion(&id, true).expect("paired capacity");
        task.receipt_tx = publisher.clone();
        let receipt_owner = crate::receipt_worker::spawn_receipt_worker(
            daemon.clone(),
            receiver,
            task.receipt_map.clone(),
            task.outbound_resource_map.clone(),
            publisher.clone(),
        );
        let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
            queue_capacity: 2,
            ..Default::default()
        });
        let global = Arc::new(Semaphore::new(1));
        let peer_limit = Arc::new(Semaphore::new(1));
        let stamp = Arc::new(Semaphore::new(1));
        let (stop, stopping) = tokio::sync::watch::channel(false);
        let owner = tokio::spawn(completion::complete_delivery(
            admitted(&scheduler, task),
            global.clone(),
            peer_limit.clone(),
            stamp.clone(),
            scheduler.metrics.clone(),
            peer,
            stopping.clone(),
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            while sender.capacity() != 1 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("worker took custody event while terminal remains reserved");
        let wire = daemon
            .durable_prepared_payload(&id)
            .expect("wire lookup")
            .flatten()
            .expect("prepared wire");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!owner.is_finished());
        assert!(scheduler.owns_delivery(&id));
        assert!(scheduler.reserve_message(&id, 4096).expect("duplicate").is_none());
        assert_eq!(scheduler.backlog_limit.available_permits(), 1);
        assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize - 4096);
        assert_eq!(state(&connection, &id), "scheduled");
        assert_eq!(global.available_permits(), 1);
        assert_eq!(peer_limit.available_permits(), 1);
        assert_eq!(stamp.available_permits(), 1);

        // Exercise a real second peer handoff while the first receipt cannot commit.
        let mut other =
            delivery_task_for_propagation_cost_lookup(Arc::new(RpcDaemon::test_instance()));
        let remote = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
        other.peer_identity = Some(*remote.as_identity());
        other.transport = transport;
        other.message_id = "other-during-custody".into();
        other.requested_method = RequestedDeliveryMethod::Opportunistic;
        other.stamp_cost = Some(0);
        other.destination_hash = rns_transport::destination::SingleOutputDestination::new(
            *remote.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        other.destination = other.destination_hash.as_slice().try_into().expect("destination");
        other.destination_hex = hex::encode(other.destination);
        let destination = other.destination_hash;
        let mut remote_destination =
            SingleInputDestination::new(remote, DestinationName::new("lxmf", "delivery"));
        channel
            .rx_channel
            .send(RxMessage {
                address: *channel.address(),
                packet: remote_destination.announce(OsRng, None).expect("peer announce"),
                source: IfaceSource::None,
            })
            .await
            .expect("announce ingress");
        tokio::time::timeout(Duration::from_secs(3), async {
            while !other.transport.has_path(&destination).await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("second peer path");
        let other_peer = other.destination_hex.clone();
        let (other_stop, other_stopping) = tokio::sync::watch::channel(false);
        let second = tokio::spawn(completion::complete_delivery(
            admitted(&scheduler, other),
            global.clone(),
            Arc::new(Semaphore::new(1)),
            stamp,
            scheduler.metrics.clone(),
            other_peer,
            other_stopping,
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let frame = channel.tx_channel.recv().await.expect("second peer frame");
                if frame.packet.destination == destination
                    && frame.packet.header.packet_type == PacketType::Data
                {
                    break;
                }
            }
        })
        .await
        .expect("second peer sends despite custody storage fault");
        other_stop.send_replace(true);
        second.await.expect("second owner stops");
        assert!(!owner.is_finished());
        assert_eq!(
            daemon.durable_prepared_payload(&id).expect("wire").flatten(),
            Some(wire.clone())
        );
        if shutdown_before_commit {
            stop.send_replace(true);
            tokio::time::timeout(Duration::from_secs(2), owner)
                .await
                .expect("shutdown deadline")
                .expect("owner stops");
            assert_eq!(state(&connection, &id), "scheduled", "uncommitted custody cannot requeue");
            connection.execute_batch("DROP TRIGGER fail_custody;").expect("restore storage");
        } else {
            connection.execute_batch("DROP TRIGGER fail_custody;").expect("restore storage");
            tokio::time::timeout(Duration::from_secs(3), owner)
                .await
                .expect("commit releases owner")
                .expect("owner");
        }
        tokio::time::timeout(Duration::from_secs(3), receipt_owner.drain())
            .await
            .expect("drain deadline")
            .expect("drained");
        assert_eq!(state(&connection, &id), "custody");
        assert_eq!(sender.capacity(), 2, "unused transient permits retire");
        assert!(!scheduler.owns_delivery(&id));
        assert_eq!(scheduler.backlog_limit.available_permits(), 2);
        assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
        assert_eq!(daemon.durable_prepared_payload(&id).expect("wire").flatten(), Some(wire));
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .expect("journal count"),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM broker_completions WHERE message_id=?1",
                    [&id],
                    |r| r.get::<_, i64>(0)
                )
                .expect("terminal reserve"),
            1
        );
        scheduler.shutdown();
    }
}
