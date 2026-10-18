use super::DeliverySchedulerMetrics;
use crate::bridge::delivery_attempt::DeliveryAttemptOutcome;
use crate::bridge::delivery_task::DeliveryTask;
use crate::bridge::delivery_timing::measured_delivery_timing;
use lxmf::message::{DeliverySchedule, DeliveryTiming};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Wait,
    Send,
    Request,
    Rediscover,
    Fail,
}

#[derive(Clone, Copy, Debug, Default)]
struct Retry {
    schedule: DeliverySchedule,
    attempts: u32,
    sent_on_path: bool,
}

impl Retry {
    fn next(&mut self, now: Duration, path: bool, timing: DeliveryTiming) -> Action {
        if !self.schedule.due(now, path) {
            return Action::Wait;
        }
        if self.attempts > self.schedule.max_attempts() {
            return Action::Fail;
        }
        self.schedule.begin_attempt();
        let action = if self.attempts >= 1 && !path {
            self.sent_on_path = false;
            self.schedule.schedule_path(now, timing);
            Action::Request
        } else if self.attempts == 2 && path && self.sent_on_path {
            self.sent_on_path = false;
            self.schedule.schedule_rediscovery(now, timing);
            Action::Rediscover
        } else {
            self.sent_on_path = path;
            self.schedule.schedule_delivery(now, timing);
            Action::Send
        };
        self.attempts += 1;
        action
    }
}

/// Runs within the scheduler's retained admission and prepared-message owner.
/// Only individual sends hold execution permits; no delayed work is detached.
pub(super) async fn run(
    task: &DeliveryTask,
    payload: &[u8],
    global: Arc<Semaphore>,
    peer_limit: Arc<Semaphore>,
    metrics: &DeliverySchedulerMetrics,
    peer: &str,
    stopping: &mut watch::Receiver<bool>,
) {
    let origin = Instant::now();
    let mut retry = Retry::default();
    let mut initialized = false;
    let mut forced_request_due = None;
    let mut completion = task.receipt_tx.terminal_completion_events();
    task.wakeup.wait_for(task.destination_hash);
    loop {
        match ready(task, stopping) {
            Ok(false) => return,
            Err(error) => {
                log::warn!(
                    "opportunistic owner observation failed message_id={}: {error}",
                    task.message_id
                );
                wait(task, stopping, &mut completion, Duration::from_millis(250)).await;
                continue;
            }
            Ok(true) => {}
        }
        if forced_request_due.is_some_and(|due| Instant::now() >= due) {
            forced_request_due = None;
            // This request was claimed before path expiry. Ordinary debounce
            // here would suppress the request against its own timestamp.
            task.transport.request_path(&task.destination_hash, None, None).await;
            continue;
        }
        let measurements = task.transport.path_timing(&task.destination_hash).await;
        let timing = match measured_delivery_timing(measurements) {
            Ok(timing) => timing,
            Err(error) => {
                task.fail_payload_build(std::io::Error::other(error));
                return;
            }
        };
        match ready(task, stopping) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!(
                    "opportunistic observation after timing failed message_id={}: {error}",
                    task.message_id
                );
                wait(task, stopping, &mut completion, Duration::from_millis(250)).await;
                continue;
            }
        }
        if !initialized {
            initialized = true;
            if !measurements.path_known {
                let Ok(timing) = task.request_delivery_path(task.destination_hash).await else {
                    return;
                };
                retry.schedule.schedule_path(origin.elapsed(), timing);
                continue;
            }
        }
        let mut proposed = retry;
        let mut action = proposed.next(origin.elapsed(), measurements.path_known, timing);
        let mut permits = None;
        if action == Action::Send {
            permits = match acquire(
                task,
                stopping,
                &mut completion,
                &global,
                &peer_limit,
                forced_request_due,
            )
            .await
            {
                Acquisition::Ready(permits) => Some(permits),
                Acquisition::RediscoveryDue => continue,
                Acquisition::Stopped => return,
            };
            // Permit waiting can outlive a receipt or path change. Re-evaluate
            // before consuming an attempt or choosing its sent_on_path value.
            match ready(task, stopping) {
                Ok(true) => {}
                Ok(false) => return,
                Err(_) => {
                    drop(permits);
                    continue;
                }
            }
            let measurements = task.transport.path_timing(&task.destination_hash).await;
            let timing = match measured_delivery_timing(measurements) {
                Ok(timing) => timing,
                Err(error) => {
                    task.fail_payload_build(std::io::Error::other(error));
                    return;
                }
            };
            proposed = retry;
            action = proposed.next(origin.elapsed(), measurements.path_known, timing);
            if action != Action::Send {
                drop(permits.take());
            }
        }
        match action {
            Action::Wait => {}
            Action::Fail => {
                task.fail_payload_build(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("opportunistic delivery exhausted for {}", task.destination_hex),
                ));
                return;
            }
            Action::Request => {
                let Ok(timing) = task.request_delivery_path(task.destination_hash).await else {
                    return;
                };
                proposed.schedule.schedule_path(origin.elapsed(), timing);
                retry = proposed;
                continue;
            }
            Action::Rediscover => {
                match task.path_requests.claim_rediscovery(task.destination_hash, timing) {
                    Ok(true) => {
                        task.transport.expire_path(&task.destination_hash).await;
                        forced_request_due = Some(Instant::now() + Duration::from_millis(500));
                    }
                    Ok(false) => {}
                    Err(error) => {
                        task.fail_payload_build(error);
                        return;
                    }
                }
                let timing = match measured_delivery_timing(
                    task.transport.path_timing(&task.destination_hash).await,
                ) {
                    Ok(timing) => timing,
                    Err(error) => {
                        task.fail_payload_build(std::io::Error::other(error));
                        return;
                    }
                };
                proposed.schedule.schedule_rediscovery(origin.elapsed(), timing);
                retry = proposed;
                continue;
            }
            Action::Send => {
                metrics.record_dequeued_for_peer(peer);
                metrics.record_started_for_peer(peer);
                let outcome = task.run_opportunistic(payload).await;
                drop(permits);
                metrics.record_waiting_for_peer(peer);
                match outcome {
                    DeliveryAttemptOutcome::OpportunisticPacket => retry = proposed,
                    DeliveryAttemptOutcome::AwaitPath { destination } => {
                        // Missing identity is a Rust admission extension. Keep
                        // consumed attempts instead of starting a new discovery budget.
                        let Ok(timing) = task.request_delivery_path(destination).await else {
                            return;
                        };
                        proposed.schedule.schedule_path(origin.elapsed(), timing);
                        retry = proposed;
                    }
                    DeliveryAttemptOutcome::AwaitStatus => {
                        wait(task, stopping, &mut completion, Duration::from_millis(250)).await;
                        continue;
                    }
                    _ => return, // Link/Resource fallback never enters this resend loop.
                }
            }
        }
        let mut delay = retry.schedule.deadline().map_or(Duration::from_millis(250), |deadline| {
            deadline.saturating_sub(origin.elapsed()).saturating_add(Duration::from_nanos(1))
        });
        if let Some(due) = forced_request_due {
            delay = delay.min(due.saturating_duration_since(Instant::now()));
        }
        wait(task, stopping, &mut completion, delay).await;
    }
}

fn ready(task: &DeliveryTask, stopping: &watch::Receiver<bool>) -> std::io::Result<bool> {
    if *stopping.borrow() || stopping.has_changed().is_err() {
        return Ok(false);
    }
    task.observe_delivery_terminal().map(|terminal| !terminal)
}

async fn wait(
    task: &DeliveryTask,
    stopping: &mut watch::Receiver<bool>,
    completion: &mut watch::Receiver<()>,
    delay: Duration,
) {
    tokio::select! {
        _ = task.wakeup.changed.notified() => {},
        _ = completion.changed() => {},
        _ = stopping.changed() => {},
        _ = tokio::time::sleep(delay.min(Duration::from_millis(250))) => {},
    }
}

enum Acquisition {
    Ready((OwnedSemaphorePermit, OwnedSemaphorePermit)),
    RediscoveryDue,
    Stopped,
}

async fn acquire(
    task: &DeliveryTask,
    stopping: &mut watch::Receiver<bool>,
    completion: &mut watch::Receiver<()>,
    global: &Arc<Semaphore>,
    peer: &Arc<Semaphore>,
    forced_request_due: Option<Instant>,
) -> Acquisition {
    let permits = async {
        let peer = peer.clone().acquire_owned().await?;
        let global = global.clone().acquire_owned().await?;
        Ok::<_, tokio::sync::AcquireError>((peer, global))
    };
    tokio::pin!(permits);
    loop {
        match ready(task, stopping) {
            Ok(false) => return Acquisition::Stopped,
            Err(error) => {
                log::warn!(
                    "opportunistic permit observation failed message_id={}: {error}",
                    task.message_id
                );
                wait(task, stopping, completion, Duration::from_millis(250)).await;
                continue;
            }
            Ok(true) => {}
        }
        tokio::select! {
            result = &mut permits => return match result {
                Ok(permits) => Acquisition::Ready(permits),
                Err(_) => Acquisition::Stopped,
            },
            _ = async {
                match forced_request_due {
                    Some(due) => tokio::time::sleep_until(due).await,
                    None => std::future::pending().await,
                }
            } => return Acquisition::RediscoveryDue,
            _ = wait(task, stopping, completion, Duration::from_millis(250)) => {},
        }
    }
}

#[cfg(test)]
#[path = "bridge_delivery_retry_tests.rs"]
mod tests;
