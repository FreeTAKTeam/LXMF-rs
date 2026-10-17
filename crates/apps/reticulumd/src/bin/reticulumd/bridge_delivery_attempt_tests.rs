use super::*;
use crate::bridge::delivery_attempt::DeliveryAttemptOutcome;
use crate::bridge::delivery_task::PreparedDeliveryPayload;
use crate::bridge::delivery_task_tests::task_with_active_link;

fn committed_wire(task: &DeliveryTask) -> serde_json::Value {
    let value = task
        .daemon
        .durable_prepared_payload(&task.message_id)
        .expect("durable lookup")
        .expect("durable operation")
        .expect("committed wire");
    encode_prepared(&decode_prepared(value).expect("valid committed wire"))
}

async fn fallback_task() -> (
    tempfile::TempDir,
    rusqlite::Connection,
    DeliveryTask,
    PreparedDeliveryPayload,
    tokio::sync::mpsc::Receiver<ReceiptEvent>,
) {
    let (mut task, _link, receipts) = task_with_active_link().await;
    let (directory, connection, durable) = durable_task_for_destination(task.destination);
    task.daemon = durable.daemon;
    task.message_id = durable.message_id;
    task.stamp_cost = Some(0);
    task.try_propagation_on_fail = true;
    let local_node = hex::encode([0x77; 16]);
    task.propagation_node_hex = Some(local_node.clone());
    task.daemon.set_propagation_destination_hash(Some(local_node.clone()));
    task.daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "propagation_enable".into(),
            params: Some(json!({"enabled": true, "target_cost": 1, "stamp_cost_flexibility": 0})),
        })
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec(&rmpv::Value::Array(vec![
        false.into(),
        1_700_000_803i64.into(),
        true.into(),
        256.into(),
        2048.into(),
        rmpv::Value::Array(vec![1.into(), 0.into(), 0.into()]),
        rmpv::Value::Map(Vec::new()),
    ]))
    .expect("announce data");
    task.daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "announce_received".into(),
            params: Some(json!({"peer": local_node, "timestamp": 1_700_000_803i64,
            "app_data_hex": hex::encode(app_data), "aspect": "lxmf.propagation", "hops": 0})),
        })
        .expect("announce");
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("allow initial commit");
    let prepared = PreparedDeliveryPayload {
        lxmf_payload: task.build_payload().await.expect("LXMF payload"),
        propagation: None,
    };
    task.daemon
        .persist_durable_prepared_payload(&task.message_id, encode_prepared(&prepared))
        .expect("initial LXMF commit");
    connection.execute_batch("CREATE TRIGGER fail_prepared BEFORE UPDATE OF prepared ON broker_operations BEGIN SELECT RAISE(ABORT,'injected fallback commit failure'); END;").expect("fallback fault");
    (directory, connection, task, prepared, receipts)
}

#[tokio::test]
async fn borrowed_fallback_commits_before_reuse_and_restores_exact_envelope() {
    let (_directory, connection, task, mut prepared, mut receipts) = fallback_task().await;
    let stamp = Arc::new(Semaphore::new(1));
    let initial = encode_prepared(&prepared);
    assert_eq!(
        task.run_prepared_attempt(&mut prepared, &stamp).await,
        DeliveryAttemptOutcome::DeferredStorage
    );
    assert!(prepared.propagation.is_none(), "uncommitted envelope must not become reusable");
    assert_eq!(committed_wire(&task), initial);
    assert_eq!(state(&connection, &task.message_id), "scheduled");
    assert!(task.receipt_map.lock().expect("receipts").is_empty());
    assert!(task.outbound_resource_map.lock().expect("resources").is_empty());
    while let Ok(receipt) = receipts.try_recv() {
        assert!(receipt.status.starts_with("link failed:"), "no propagation send: {receipt:?}");
    }
    let status = task
        .daemon
        .handle_rpc(RpcRequest { id: 3, method: "propagation_status".into(), params: None })
        .expect("status")
        .result
        .expect("result");
    assert_ne!(status["propagation"]["last_ingest_count"].as_u64(), Some(1));

    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("recover storage");
    assert_eq!(
        task.run_prepared_attempt(&mut prepared, &stamp).await,
        DeliveryAttemptOutcome::CompletedLocal
    );
    let committed = encode_prepared(&prepared);
    assert_ne!(committed, initial);
    assert_eq!(committed_wire(&task), committed);
    // A closed stamp lane makes an accidental rebuild fail immediately.
    stamp.close();
    assert_eq!(
        task.run_prepared_attempt(&mut prepared, &stamp).await,
        DeliveryAttemptOutcome::CompletedLocal
    );
    assert_eq!(encode_prepared(&prepared), committed);
    prepared.propagation = None;
    assert_eq!(
        task.run_prepared_attempt(&mut prepared, &stamp).await,
        DeliveryAttemptOutcome::CompletedLocal
    );
    assert_eq!(
        encode_prepared(&prepared),
        committed,
        "restore exact destination, cost, ciphertext and transient ID"
    );
}

#[tokio::test]
async fn scheduler_fallback_commit_failure_releases_before_requeue() {
    let (_directory, connection, task, prepared, _receipts) = fallback_task().await;
    let initial = encode_prepared(&prepared);
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
    completion::complete_delivery(
        delivery,
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        scheduler.metrics.clone(),
        peer,
        stopping,
    )
    .await;
    assert_eq!(state(&connection, &id), "queued");
    assert!(scheduler.owned_messages.lock().expect("owners").is_empty());
    assert_eq!(scheduler.backlog_limit.available_permits(), 1);
    assert_eq!(scheduler.bytes.available_permits(), DELIVERY_RETAINED_BYTES as usize);
    let stored =
        daemon.durable_prepared_payload(&id).expect("durable").flatten().expect("committed");
    assert_eq!(encode_prepared(&decode_prepared(stored).expect("valid wire")), initial);
    scheduler.shutdown();
}
