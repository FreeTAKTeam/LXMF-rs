use super::{prepare_payload, DeliverySchedulerMetrics, ScheduledDelivery};
use crate::bridge::delivery_task::DeliveryTask;
use crate::bridge::RequestedDeliveryMethod;
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
    run_delivery(delivery, global_limit, peer_limit, stamp_limit, metrics, peer, stopping.clone())
        .await;
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
    mut stopping: watch::Receiver<bool>,
) {
    // Release membership last, after retained capacity and task state.
    let _ownership = delivery._ownership;
    let _capacity = delivery._capacity_permit;
    let _bytes = delivery._bytes;
    let mut task = delivery.task;
    task.stopping = Some(stopping.clone());
    run_attempts(&mut task, global_limit, peer_limit, stamp_limit, metrics, &peer, &mut stopping)
        .await;
    await_terminal_commit(&task, &mut stopping).await;
}

/// Fence accepted terminal publication, not transport handoff or unused receipt
/// capacity. The receipt worker remains the only publisher/persistence owner.
async fn await_terminal_commit(task: &DeliveryTask, stopping: &mut watch::Receiver<bool>) {
    let mut completion = task.receipt_tx.terminal_completion_events();
    loop {
        match task.receipt_tx.terminal_pending(&task.message_id) {
            Ok(false) => return,
            Ok(true) => {},
            Err(error) => log::error!(
                "terminal persistence fence unavailable message_id={}; retaining delivery owner: {error}", task.message_id
            ),
        }
        if *stopping.borrow() || stopping.has_changed().is_err() {
            return;
        }
        tokio::select! {
            _ = completion.changed() => {},
            _ = stopping.changed() => {},
        }
    }
}

async fn run_attempts(
    task: &mut DeliveryTask,
    global_limit: Arc<Semaphore>,
    peer_limit: Arc<Semaphore>,
    stamp_limit: Arc<Semaphore>,
    metrics: Arc<DeliverySchedulerMetrics>,
    peer: &str,
    stopping: &mut watch::Receiver<bool>,
) {
    let prepared = prepare_payload(task, &stamp_limit, &metrics, peer).await;
    let Some(mut prepared) = prepared else {
        metrics.record_finished_before_delivery_for_peer(peer);
        return;
    };
    run_prepared(
        task,
        &mut prepared,
        super::DeliveryLimits { global: global_limit, peer: peer_limit, stamp: stamp_limit },
        metrics,
        peer,
        stopping,
    )
    .await;
}

pub(in crate::bridge) async fn run_prepared(
    task: &mut DeliveryTask,
    prepared: &mut crate::bridge::delivery_task::PreparedDeliveryPayload,
    limits: super::DeliveryLimits,
    metrics: Arc<DeliverySchedulerMetrics>,
    peer: &str,
    stopping: &mut watch::Receiver<bool>,
) {
    if prepared.propagation.is_some() {
        // A committed fallback envelope fixes the effective method after restart.
        task.requested_method = RequestedDeliveryMethod::Propagated;
    }
    if task.requested_method == RequestedDeliveryMethod::Opportunistic
        && crate::bridge_helpers::opportunistic_payload(&prepared.lxmf_payload, &task.destination)
            .len()
            <= lxmf::constants::ENCRYPTED_PACKET_MDU
    {
        let needs_link = super::opportunistic::run(
            task,
            &prepared.lxmf_payload,
            limits.global.clone(),
            limits.peer.clone(),
            &metrics,
            peer,
            stopping,
        )
        .await;
        if !needs_link {
            metrics.record_finished_before_delivery_for_peer(peer);
            return;
        }
    }
    super::link_owner::run(task, prepared, limits, &metrics, peer, stopping).await;
    metrics.record_finished_before_delivery_for_peer(peer);
}

pub(super) async fn await_custody_commit(
    task: &DeliveryTask,
    event: reticulum_daemon::receipt_bridge::ReceiptEvent,
    stopping: &mut watch::Receiver<bool>,
) {
    // The native custody action has already succeeded. Retry only publication,
    // never ingestion or transmission, while retaining admission and signed bytes.
    loop {
        match task.receipt_tx.publish_custody(event.clone()) {
            Ok(mut completion) => {
                if *stopping.borrow() || stopping.has_changed().is_err() {
                    return;
                }
                tokio::select! {
                    result = completion.wait() => {
                        if let Err(error) = result {
                            log::error!("custody persistence unavailable message_id={}; retaining owner until shutdown: {error}", task.message_id);
                            wait_for_shutdown(stopping).await;
                        }
                    },
                    _ = stopping.changed() => {},
                }
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if *stopping.borrow() || stopping.has_changed().is_err() {
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                    _ = stopping.changed() => {},
                }
            }
            Err(error) => {
                log::error!("custody publication unavailable message_id={}; retaining owner until shutdown: {error}", task.message_id);
                wait_for_shutdown(stopping).await;
                return;
            }
        }
    }
}

pub(super) async fn wait_for_shutdown(stopping: &mut watch::Receiver<bool>) {
    while !*stopping.borrow() {
        if stopping.changed().await.is_err() {
            break;
        }
    }
}

pub(super) async fn publish_terminal(
    task: &DeliveryTask,
    event: reticulum_daemon::receipt_bridge::ReceiptEvent,
    stopping: &mut watch::Receiver<bool>,
) {
    loop {
        match task.receipt_tx.try_send(event.clone()) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if *stopping.borrow() || stopping.has_changed().is_err() {
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                    _ = stopping.changed() => {},
                }
            }
            Err(error) => {
                log::error!("native terminal publication unavailable message_id={}; retaining owner until shutdown: {error}", task.message_id);
                wait_for_shutdown(stopping).await;
                return;
            }
        }
    }
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
