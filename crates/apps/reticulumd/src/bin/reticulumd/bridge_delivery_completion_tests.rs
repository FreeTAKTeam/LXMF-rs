use super::*;
use crate::bridge::delivery_task_tests::delivery_task_for_propagation_cost_lookup;
use rns_rpc::BrokerCommand;

fn durable_task() -> (tempfile::TempDir, rusqlite::Connection, DeliveryTask) {
    durable_task_for_destination([0; 16])
}

fn durable_task_for_destination(
    destination: [u8; 16],
) -> (tempfile::TempDir, rusqlite::Connection, DeliveryTask) {
    durable_task_for_content(destination, "payload")
}

fn durable_task_for_content(
    destination: [u8; 16],
    content: &str,
) -> (tempfile::TempDir, rusqlite::Connection, DeliveryTask) {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("delivery.db");
    let store = MessagesStore::open(&path).expect("store");
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable broker");
    let admitted = store
        .broker_command(BrokerCommand::Admit {
            owner: "test-owner".into(),
            source: hex::encode([1; 16]),
            request: rns_rpc::broker::AdmitRequest {
                identity: "test-source".into(),
                operation_id: "deferred-owner".into(),
                destination: hex::encode(destination),
                title: "".into(),
                content: content.into(),
                fields: None,
                options: rns_rpc::OutboundDeliveryOptions::default(),
            },
        })
        .expect("admit");
    let id = admitted["message_id"].as_str().expect("message id");
    assert_eq!(
        store
            .broker_command(BrokerCommand::DispatchState {
                message_id: id.into(),
                state: "scheduled".into()
            })
            .expect("claim")["claimed"],
        true
    );
    let daemon = Arc::new(RpcDaemon::with_store(store, "deferred-owner".into()));
    let mut task = delivery_task_for_propagation_cost_lookup(daemon);
    task.message_id = id.into();
    task.destination = destination;
    task.destination_hash = rns_transport::hash::AddressHash::new(destination);
    task.destination_hex = hex::encode(destination);
    task.requested_method = RequestedDeliveryMethod::Direct;
    task.stamp_cost = Some(0);
    task.content = content.into();
    let connection = rusqlite::Connection::open(path).expect("connection");
    // Exercise a real preparation write failure before any network dispatch.
    connection.execute_batch("CREATE TRIGGER fail_prepared BEFORE UPDATE OF prepared ON broker_operations BEGIN SELECT RAISE(ABORT,'injected prepared write failure'); END;").expect("fault");
    (directory, connection, task)
}

fn state(connection: &rusqlite::Connection, id: &str) -> String {
    connection
        .query_row("SELECT state FROM broker_operations WHERE message_id=?1", [id], |row| {
            row.get(0)
        })
        .expect("state")
}

#[tokio::test]
async fn scheduler_requeues_only_after_releasing_message_and_capacity() {
    let (_directory, connection, task) = durable_task();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let reservation = scheduler
        .reserve_message(&task.message_id, DELIVERY_RETAINED_BYTES as usize)
        .expect("reserve")
        .expect("owner");
    let peer = task.destination_hex.clone();
    let stamp = Arc::new(Semaphore::new(1));
    assert!(prepare_payload(&task, &stamp, &scheduler.metrics, &peer).await.is_none());
    assert_eq!(
        state(&connection, &task.message_id),
        "scheduled",
        "inner preparation cannot release the durable claim"
    );
    assert!(task.deferred_dispatch.load(Ordering::Acquire));
    assert!(scheduler.reserve_message(&task.message_id, 1).expect("duplicate").is_none());
    let id = task.message_id.clone();
    scheduler.metrics.record_admitted_for_peer(&peer);
    let delivery = ScheduledDelivery {
        task,
        _capacity_permit: reservation.count,
        _bytes: reservation.bytes,
        _ownership: reservation.ownership,
    };
    let (_stop, stopping) = tokio::sync::watch::channel(false);
    completion::complete_delivery(
        delivery,
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        stamp,
        scheduler.metrics.clone(),
        peer,
        stopping,
    )
    .await;
    assert_eq!(state(&connection, &id), "queued");
    assert!(scheduler.owned_messages.lock().expect("owners").is_empty());
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
    assert_eq!(scheduler.metrics.snapshot().queued_current, 0);
    // The next broker claim can acquire a real owner, rather than a duplicate no-op.
    connection
        .execute("UPDATE broker_operations SET state='scheduled' WHERE message_id=?1", [&id])
        .expect("next claim");
    let next = scheduler
        .reserve_message(&id, DELIVERY_RETAINED_BYTES as usize)
        .expect("reserve again")
        .expect("new owner");
    drop(next);
    scheduler.shutdown();
}

#[tokio::test]
async fn scheduler_failed_requeue_keeps_retrying_without_holding_delivery_capacity() {
    for terminal in [false, true] {
        let (_directory, connection, task) = durable_task();
        connection.execute_batch("CREATE TRIGGER fail_requeue BEFORE UPDATE OF state ON broker_operations WHEN NEW.state='queued' BEGIN SELECT RAISE(ABORT,'injected requeue failure'); END;").expect("requeue fault");
        let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
            queue_capacity: 1,
            ..Default::default()
        });
        let id = task.message_id.clone();
        let daemon = task.daemon.clone();
        let peer = task.destination_hex.clone();
        let reservation = scheduler
            .reserve_message(&id, DELIVERY_RETAINED_BYTES as usize)
            .expect("reserve")
            .expect("owner");
        scheduler.metrics.record_admitted_for_peer(&peer);
        let delivery = ScheduledDelivery {
            task,
            _capacity_permit: reservation.count,
            _bytes: reservation.bytes,
            _ownership: reservation.ownership,
        };
        let (_stop, stopping) = tokio::sync::watch::channel(false);
        let completion = completion::complete_delivery(
            delivery,
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(1)),
            scheduler.metrics.clone(),
            peer,
            stopping,
        );
        let release_fault = async {
            while scheduler.metrics.snapshot().completed_total == 0 {
                tokio::task::yield_now().await;
            }
            assert_eq!(state(&connection, &id), "scheduled");
            assert!(scheduler.owned_messages.lock().expect("owners").is_empty());
            assert_eq!(scheduler.backlog_limit.available_permits(), 1);
            assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
            if terminal {
                daemon
                    .record_network_receipt(
                        &id,
                        "failed: cancelled during requeue",
                        serde_json::Map::new(),
                    )
                    .expect("terminal receipt");
            }
            connection.execute_batch("DROP TRIGGER fail_requeue;").expect("recover storage");
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(completion, release_fault);
        })
        .await
        .expect("requeue recovered");
        assert_eq!(state(&connection, &id), if terminal { "terminal" } else { "queued" });
        scheduler.shutdown();
    }
}

#[tokio::test]
async fn scheduler_enqueue_rejection_releases_identity_and_both_reservations() {
    let (_directory, _connection, task) = durable_task();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let reservation = scheduler
        .reserve_message(&task.message_id, DELIVERY_RETAINED_BYTES as usize)
        .expect("reserve")
        .expect("owner");
    scheduler.shutdown();
    assert_eq!(
        scheduler.enqueue_reserved(task, reservation).expect_err("closed queue").kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert!(scheduler.owned_messages.lock().expect("owners").is_empty());
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
    assert_eq!(scheduler.metrics.snapshot().accepted_total, 0);
    assert_eq!(scheduler.metrics.snapshot().queued_current, 0);
}

#[tokio::test]
async fn scheduler_deferred_storage_completions_remain_bounded() {
    let (_directory, connection, task) = durable_task();
    connection.execute_batch("CREATE TRIGGER fail_requeue BEFORE UPDATE OF state ON broker_operations WHEN NEW.state='queued' BEGIN SELECT RAISE(ABORT,'injected requeue failure'); END;").expect("requeue fault");
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig {
        queue_capacity: 1,
        ..Default::default()
    });
    let first_id = task.message_id.clone();
    let reservation = scheduler.reserve_message(&first_id, 1).expect("reserve").expect("owner");
    scheduler.enqueue_reserved(task, reservation).expect("enqueue first");
    tokio::time::timeout(Duration::from_secs(3), async {
        while scheduler.owned_messages.lock().expect("owners").contains_key(&first_id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first completion reached");
    let store = MessagesStore::in_memory().expect("second store");
    store
        .insert_message(&MessageRecord {
            id: "second".into(),
            source: "source".into(),
            destination: hex::encode([0; 16]),
            title: "".into(),
            content: "".into(),
            timestamp: 1,
            direction: "out".into(),
            fields: None,
            receipt_status: Some("cancelled".into()),
        })
        .expect("second message");
    let mut second = delivery_task_for_propagation_cost_lookup(Arc::new(RpcDaemon::with_store(
        store,
        "second".into(),
    )));
    second.message_id = "second".into();
    let reservation =
        scheduler.reserve_message("second", 1).expect("second reserve").expect("second owner");
    scheduler.enqueue_reserved(second, reservation).expect("enqueue second");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        scheduler.metrics.snapshot().completed_total,
        1,
        "storage completion still occupies the bounded owner slot"
    );
    assert_eq!(scheduler.metrics.snapshot().queued_current, 1);
    connection.execute_batch("DROP TRIGGER fail_requeue;").expect("recover storage");
    tokio::time::timeout(Duration::from_secs(3), async {
        while scheduler.metrics.snapshot().completed_total < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("queued task resumed");
    assert_eq!(state(&connection, &first_id), "queued");
    scheduler.shutdown();
}

#[path = "bridge_delivery_attempt_tests.rs"]
mod attempt_tests;

#[path = "bridge_delivery_owner_tests.rs"]
mod owner_tests;
