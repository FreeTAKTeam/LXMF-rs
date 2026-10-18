use super::{DeliverySchedulerMetrics, DeliveryTask};
use crate::bridge::delivery_attempt::DeliveryAttemptOutcome;
use crate::bridge::delivery_task::PreparedDeliveryPayload;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub(in crate::bridge) async fn prepare_propagation(
    task: &DeliveryTask,
    prepared: &mut PreparedDeliveryPayload,
    stamp_limit: &Arc<Semaphore>,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
) -> Result<(), DeliveryAttemptOutcome> {
    if task.stopping.as_ref().is_some_and(|stop| *stop.borrow() || stop.has_changed().is_err()) {
        return Err(DeliveryAttemptOutcome::Stopped);
    }
    match task.observe_delivery_terminal() {
        Ok(false) => {}
        Ok(true) => return Err(DeliveryAttemptOutcome::Stopped),
        Err(error) => {
            log::warn!(
                "propagation preparation observation unavailable message_id={}: {error}",
                task.message_id
            );
            return Err(DeliveryAttemptOutcome::AwaitStatus);
        }
    }
    // Both initial preparation and fallback publish this cache only after
    // the durable write succeeds. Reuse the exact envelope on later attempts.
    if prepared.propagation.is_some() {
        return Ok(());
    }
    let durable = match task.daemon.durable_prepared_payload(&task.message_id) {
        Ok(Some(Some(value))) => {
            let stored = match crate::bridge::delivery_scheduler::decode_prepared(value) {
                Ok(stored) => stored,
                Err(error) => {
                    task.fail_payload_build(error);
                    return Err(DeliveryAttemptOutcome::Terminal);
                }
            };
            if stored.lxmf_payload != prepared.lxmf_payload {
                task.fail_payload_build(std::io::Error::other(
                    "prepared LXMF bytes differ from durable message",
                ));
                return Err(DeliveryAttemptOutcome::Terminal);
            }
            if let Some(propagation) = stored.propagation {
                prepared.propagation = Some(propagation);
                return Ok(());
            }
            true
        }
        Ok(Some(None)) => true,
        Ok(None) => false,
        Err(error) => {
            log::error!(
                "durable propagation lookup failed message_id={}: {error}",
                task.message_id
            );
            return Err(DeliveryAttemptOutcome::DeferredStorage);
        }
    };
    let envelope = super::preparation::build_propagation_payload(
        task,
        &prepared.lxmf_payload,
        stamp_limit,
        metrics,
        peer,
    )
    .await?;
    if durable {
        let encoded = crate::bridge::delivery_scheduler::encode_prepared_parts(
            &prepared.lxmf_payload,
            Some(&envelope),
        );
        if let Err(error) = task.daemon.persist_durable_prepared_payload(&task.message_id, encoded)
        {
            log::error!(
                "propagation bytes were not committed; send suppressed message_id={}: {error}",
                task.message_id
            );
            return Err(DeliveryAttemptOutcome::DeferredStorage);
        }
    }
    prepared.propagation = Some(envelope);
    Ok(())
}
