use crate::bridge::delivery_attempt::DeliveryAttemptOutcome;
use crate::bridge::delivery_task::DeliveryTask;
use crate::bridge::RequestedDeliveryMethod;
use lxmf::message::DeliverySchedule;
use rns_transport::hash::AddressHash;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;

/// Wait within an admitted owner. No execution or stamp permit is held here.
pub(in crate::bridge) async fn wait_for_identity(
    task: &DeliveryTask,
    destination: AddressHash,
    stopping: &mut watch::Receiver<bool>,
) -> Result<(), DeliveryAttemptOutcome> {
    let origin = Instant::now();
    let mut schedule = DeliverySchedule::default();
    let mut attempts = 0u32;
    let mut requested = false;
    task.wakeup.wait_for(destination);
    loop {
        if *stopping.borrow() || task.abort_if_cancelled("discovery") {
            return Err(DeliveryAttemptOutcome::Stopped);
        }
        // Register the target before checking readiness. notify_one retains a
        // permit if an announce arrives between this check and the select.
        if task.discovery_ready(destination).await {
            return Ok(());
        }
        let now = origin.elapsed();
        if schedule.due(now, false) {
            // Direct/Propagated consume their first attempt before testing the
            // strict limit. Opportunistic has an initial discovery-only request.
            if requested || task.requested_method != RequestedDeliveryMethod::Opportunistic {
                attempts = attempts.saturating_add(1);
            }
            let timing = crate::bridge::delivery_timing::measured_delivery_timing(
                task.transport.path_timing(&destination).await,
            )
            .map_err(|error| {
                task.fail_payload_build(std::io::Error::other(error));
                DeliveryAttemptOutcome::Terminal
            })?;
            schedule.begin_attempt();
            schedule.schedule_path(now, timing);
            let exhausted = if task.requested_method == RequestedDeliveryMethod::Opportunistic {
                attempts > schedule.max_attempts()
            } else {
                attempts >= schedule.max_attempts()
            };
            if exhausted {
                if task.abort_if_cancelled("discovery-exhausted") {
                    return Err(DeliveryAttemptOutcome::Stopped);
                }
                task.fail_payload_build(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("identity discovery exhausted for {destination}"),
                ));
                return Err(DeliveryAttemptOutcome::Terminal);
            }
            task.request_delivery_path(destination).await?;
            requested = true;
        }
        let until_due = schedule
            .deadline()
            .map(|deadline| {
                deadline.saturating_sub(origin.elapsed()).saturating_add(Duration::from_nanos(1))
            })
            .unwrap_or(Duration::ZERO);
        tokio::select! {
            _ = task.wakeup.changed.notified() => {},
            _ = tokio::time::sleep(until_due.min(Duration::from_millis(250))) => {},
            changed = stopping.changed() => {
                if changed.is_err() || *stopping.borrow() {
                    return Err(DeliveryAttemptOutcome::Stopped);
                }
            }
        }
    }
}
