use super::{prepare_payload, prepare_propagation, DeliverySchedulerMetrics, ScheduledDelivery};
use crate::bridge::delivery_attempt::DeliveryAttemptOutcome;
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
    if prepared.propagation.is_some() {
        // A committed fallback envelope fixes the effective method after restart.
        task.requested_method = RequestedDeliveryMethod::Propagated;
    }
    if task.requested_method == RequestedDeliveryMethod::Opportunistic
        && crate::bridge_helpers::opportunistic_payload(&prepared.lxmf_payload, &task.destination)
            .len()
            <= lxmf::constants::ENCRYPTED_PACKET_MDU
    {
        super::opportunistic::run(
            task,
            &prepared.lxmf_payload,
            global_limit,
            peer_limit,
            &metrics,
            peer,
            stopping,
        )
        .await;
        metrics.record_finished_before_delivery_for_peer(peer);
        return;
    }
    loop {
        if *stopping.borrow() {
            metrics.record_finished_before_delivery_for_peer(peer);
            return;
        }
        if task.requested_method == RequestedDeliveryMethod::Opportunistic {
            match task.observe_delivery_terminal() {
                Ok(false) => {}
                Ok(true) => {
                    metrics.record_finished_before_delivery_for_peer(peer);
                    return;
                }
                Err(error) => {
                    log::warn!(
                        "opportunistic fallback observation failed message_id={}: {error}",
                        task.message_id
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                        _ = stopping.changed() => {},
                    }
                    continue;
                }
            }
        } else if task.abort_if_cancelled("owner") {
            metrics.record_finished_before_delivery_for_peer(peer);
            return;
        }
        let preparation = if task.requested_method == RequestedDeliveryMethod::Propagated {
            prepare_propagation(task, &mut prepared, &stamp_limit, &metrics, peer).await
        } else {
            Ok(())
        };
        let outcome = match preparation {
            Err(outcome) => outcome,
            Ok(()) => {
                let permits = async {
                    let peer = peer_limit.clone().acquire_owned().await?;
                    let global = global_limit.clone().acquire_owned().await?;
                    Ok::<_, tokio::sync::AcquireError>((peer, global))
                };
                let permits = tokio::select! {
                    result = permits => result.ok(),
                    _ = stopping.changed() => None,
                };
                let Some(permits) = permits else {
                    metrics.record_finished_before_delivery_for_peer(peer);
                    return;
                };
                metrics.record_dequeued_for_peer(peer);
                metrics.record_started_for_peer(peer);
                let outcome = task.run_prepared_attempt(&mut prepared).await;
                drop(permits);
                if matches!(
                    outcome,
                    DeliveryAttemptOutcome::AwaitPath { .. }
                        | DeliveryAttemptOutcome::FallbackPropagation
                        | DeliveryAttemptOutcome::AwaitStatus
                ) {
                    metrics.record_waiting_for_peer(peer);
                } else {
                    if outcome == DeliveryAttemptOutcome::DeferredStorage {
                        task.deferred_dispatch.store(true, std::sync::atomic::Ordering::Release);
                    }
                    metrics.record_completed_for_peer(peer);
                    return;
                }
                outcome
            }
        };
        match outcome {
            DeliveryAttemptOutcome::AwaitStatus => {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                    _ = stopping.changed() => {},
                }
            }
            DeliveryAttemptOutcome::AwaitPath { destination } => {
                if super::discovery::wait_for_identity(task, destination, stopping).await.is_err() {
                    metrics.record_finished_before_delivery_for_peer(peer);
                    return;
                }
            }
            DeliveryAttemptOutcome::FallbackPropagation => {
                // This transition is one-way for the lifetime of this owner.
                task.requested_method = RequestedDeliveryMethod::Propagated;
            }
            DeliveryAttemptOutcome::DeferredStorage => {
                task.deferred_dispatch.store(true, std::sync::atomic::Ordering::Release);
                metrics.record_finished_before_delivery_for_peer(peer);
                return;
            }
            _ => {
                metrics.record_finished_before_delivery_for_peer(peer);
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
