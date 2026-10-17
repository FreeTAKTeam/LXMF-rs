use super::{prepare_payload, DeliverySchedulerMetrics, ScheduledDelivery};
use rns_rpc::RpcDaemon;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Semaphore};

pub(super) async fn complete_delivery(
    delivery: ScheduledDelivery,
    global_limit: Arc<Semaphore>,
    peer_limit: Arc<Semaphore>,
    stamp_limit: Arc<Semaphore>,
    metrics: Arc<DeliverySchedulerMetrics>,
    peer: String,
    stopping: watch::Receiver<bool>,
) {
    let daemon = delivery.task.daemon.clone();
    let message_id = delivery.task.message_id.clone();
    let deferred = delivery.task.deferred_dispatch.clone();
    run_delivery(delivery, global_limit, peer_limit, stamp_limit, metrics, peer).await;
    // Release all task state and permits before the broker can claim again.
    if deferred.load(std::sync::atomic::Ordering::Acquire) {
        defer_dispatch(daemon.as_ref(), &message_id, stopping).await;
    }
}

async fn run_delivery(
    delivery: ScheduledDelivery,
    global_limit: Arc<Semaphore>,
    peer_limit: Arc<Semaphore>,
    stamp_limit: Arc<Semaphore>,
    metrics: Arc<DeliverySchedulerMetrics>,
    peer: String,
) {
    // Release membership last, after retained capacity and task state.
    let _ownership = delivery._ownership;
    let _capacity = delivery._capacity_permit;
    let _bytes = delivery._bytes;
    let task = delivery.task;
    let prepared = prepare_payload(&task, &stamp_limit, &metrics, &peer).await;
    let Some(prepared) = prepared else {
        metrics.record_finished_before_delivery_for_peer(&peer);
        return;
    };
    let Ok(_global_permit) = global_limit.acquire_owned().await else {
        return;
    };
    let Ok(_peer_permit) = peer_limit.acquire_owned().await else {
        return;
    };
    metrics.record_dequeued_for_peer(&peer);
    metrics.record_started_for_peer(&peer);
    task.run_prepared(prepared, stamp_limit).await;
    metrics.record_completed_for_peer(&peer);
}

pub(super) async fn defer_dispatch(
    daemon: &RpcDaemon,
    message_id: &str,
    mut stopping: watch::Receiver<bool>,
) {
    loop {
        match daemon.defer_durable_dispatch(message_id) {
            Ok(()) => return,
            Err(error) => log::error!(
                "durable dispatch requeue failed message_id={message_id}; retrying: {error}"
            ),
        }
        if *stopping.borrow() || stopping.has_changed().is_err() {
            log::error!("durable dispatch remains scheduled at shutdown message_id={message_id}; startup recovery must reclaim it");
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
            _ = stopping.changed() => {},
        }
    }
}
