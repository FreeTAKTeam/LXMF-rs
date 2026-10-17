use super::{DeliverySchedulerMetrics, DeliveryTask};
use crate::bridge::delivery_task::{
    emit_receipt_event, PreparedDeliveryPayload, PreparedPropagationPayload,
};
use crate::bridge::{log_delivery_trace, propagation};
use reticulum_daemon::receipt_bridge::ReceiptEvent;
use rns_core::destination_hash::parse_destination_hash_required;
use rns_transport::hash::AddressHash;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub(super) async fn prepare_payload(
    task: &DeliveryTask,
    stamp_limit: &Arc<Semaphore>,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
) -> Option<PreparedDeliveryPayload> {
    let finish_queued_stamp = || {
        if task.requires_normal_deferred_stamp_work() {
            metrics.record_stamp_started_for_peer(peer);
            metrics.record_stamp_completed_for_peer(peer);
        }
    };
    let durable = match task.daemon.durable_prepared_payload(&task.message_id) {
        Ok(Some(Some(value))) => {
            finish_queued_stamp();
            return match decode_prepared(value) {
                Ok(value) => Some(value),
                Err(error) => {
                    log::error!(
                        "corrupt durable prepared payload message_id={}: {error}",
                        task.message_id
                    );
                    task.fail_payload_build(std::io::Error::other(
                        "corrupt durable prepared payload",
                    ));
                    None
                }
            };
        }
        Ok(Some(None)) => true,
        Ok(None) => false,
        Err(error) => {
            finish_queued_stamp();
            log::error!("prepared payload lookup failed message_id={}: {error}", task.message_id);
            task.deferred_dispatch.store(true, std::sync::atomic::Ordering::Release);
            return None;
        }
    };
    task.start_delivery_trace();
    if task.abort_if_cancelled("start") {
        task.record_deferred_stamp_cancelled_metadata();
        if task.requires_normal_deferred_stamp_work() {
            metrics.record_stamp_started_for_peer(peer);
            metrics.record_stamp_completed_for_peer(peer);
        }
        return None;
    }

    let lxmf_payload = prepare_lxmf_payload(task, stamp_limit, metrics, peer).await?;
    let prepared = PreparedDeliveryPayload { lxmf_payload, propagation: None };
    if durable {
        if let Err(error) = task
            .daemon
            .persist_durable_prepared_payload(&task.message_id, encode_prepared(&prepared))
        {
            log::error!("prepared wire payload was not committed; network send suppressed message_id={}: {error}",task.message_id);
            task.deferred_dispatch.store(true, std::sync::atomic::Ordering::Release);
            return None;
        }
    }
    Some(prepared)
}

async fn prepare_lxmf_payload(
    task: &DeliveryTask,
    stamp_limit: &Arc<Semaphore>,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
) -> Option<Vec<u8>> {
    let normal_stamp_work = task.requires_normal_deferred_stamp_work();
    let attempts = if normal_stamp_work { 2 } else { 1 };
    for attempt in 1..=attempts {
        let _stamp_permit = if normal_stamp_work {
            let permit = match task.acquire_stamp_permit(stamp_limit).await {
                Ok(Some(permit)) => permit,
                Ok(None) => {
                    metrics.record_stamp_unqueued_for_peer(peer);
                    task.record_deferred_stamp_cancelled_metadata();
                    return None;
                }
                Err(error) => {
                    metrics.record_stamp_unqueued_for_peer(peer);
                    task.fail_payload_build(std::io::Error::other(format!(
                        "stamp worker stopped: {error}"
                    )));
                    return None;
                }
            };
            metrics.record_stamp_started_for_peer(peer);
            Some(permit)
        } else {
            None
        };
        task.record_deferred_stamp_attempt_metadata(attempt);
        let result = task.build_payload().await;
        drop(_stamp_permit);
        if normal_stamp_work {
            metrics.record_stamp_completed_for_peer(peer);
        }
        match result {
            Ok(payload) => return Some(payload),
            Err(err) => {
                if task.abort_if_cancelled("payload") {
                    task.record_deferred_stamp_cancelled_metadata();
                    return None;
                }
                if attempt < attempts {
                    task.record_deferred_stamp_retry_metadata(attempt, err.to_string());
                    metrics.record_stamp_retry_for_peer(peer);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                task.record_deferred_stamp_failed_metadata(attempt, err.to_string());
                task.fail_payload_build(err);
                return None;
            }
        }
    }
    None
}

pub(super) async fn build_propagation_payload(
    task: &DeliveryTask,
    lxmf_payload: &[u8],
    stamp_limit: &Arc<Semaphore>,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
) -> Result<PreparedPropagationPayload, super::super::delivery_attempt::DeliveryAttemptOutcome> {
    use super::super::delivery_attempt::DeliveryAttemptOutcome;
    let context = task.propagation_preparation_context().await?;
    metrics.record_stamp_queued_for_peer(peer);
    log_delivery_trace(
        &task.message_id,
        &task.destination_hex,
        "propagation",
        "building propagation payload",
    );
    task.record_propagation_stamp_work_metadata("queued", context.target_cost, None);
    for attempt in 1..=2u32 {
        let _stamp_permit = match task.acquire_stamp_permit(stamp_limit).await {
            Ok(Some(permit)) => permit,
            Ok(None) => {
                metrics.record_stamp_unqueued_for_peer(peer);
                task.record_propagation_stamp_work_metadata("cancelled", context.target_cost, None);
                return Err(DeliveryAttemptOutcome::Stopped);
            }
            Err(error) => {
                log::error!(
                    "[daemon-delivery] stamp worker stopped message_id={} peer={peer}: {error}",
                    task.message_id
                );
                task.record_propagation_stamp_work_metadata(
                    "worker_stopped",
                    context.target_cost,
                    None,
                );
                metrics.record_stamp_unqueued_for_peer(peer);
                emit_receipt_event(
                    &task.receipt_tx,
                    ReceiptEvent::new(task.message_id.clone(), "failed: stamp worker stopped")
                        .with_method("propagation"),
                );
                return Err(DeliveryAttemptOutcome::Terminal);
            }
        };
        metrics.record_stamp_started_for_peer(peer);
        task.record_propagation_stamp_attempt_metadata(context.target_cost, attempt);
        if task.abort_if_cancelled("propagation") {
            task.record_propagation_stamp_work_metadata("cancelled", context.target_cost, None);
            metrics.record_stamp_completed_for_peer(peer);
            return Err(DeliveryAttemptOutcome::Stopped);
        }
        let result = propagation::build_propagation_payload_until_cancelled(
            lxmf_payload,
            &context.destination_identity,
            context.target_cost,
            || task.abort_if_cancelled("propagation-stamp"),
        );
        drop(_stamp_permit);
        metrics.record_stamp_completed_for_peer(peer);
        match result {
            Ok(payload) => {
                task.record_propagation_stamp_work_metadata(
                    "ready",
                    context.target_cost,
                    Some(payload.stamp_value.to_string()),
                );
                task.record_propagation_payload_metadata(&payload, context.target_cost);
                return Ok(PreparedPropagationPayload {
                    propagation_node_hex: context.propagation_node_hex,
                    propagation_hash: context.propagation_hash,
                    target_cost: context.target_cost,
                    payload,
                });
            }
            Err(err) => {
                if task.abort_if_cancelled("propagation") {
                    task.record_propagation_stamp_work_metadata(
                        "cancelled",
                        context.target_cost,
                        None,
                    );
                    return Err(DeliveryAttemptOutcome::Stopped);
                }
                if attempt < 2 {
                    task.record_propagation_stamp_retry_metadata(
                        context.target_cost,
                        attempt,
                        err.to_string(),
                    );
                    metrics.record_stamp_retry_for_peer(peer);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                task.record_propagation_stamp_work_metadata(
                    "failed",
                    context.target_cost,
                    Some(err.to_string()),
                );
                task.fail_payload_build(err);
                return Err(DeliveryAttemptOutcome::Terminal);
            }
        }
    }
    Err(DeliveryAttemptOutcome::Terminal)
}

// Durable signed LXMF bytes (and any prepared propagation envelope) precede the first network send.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredPreparedPayload {
    lxmf_hex: String,
    propagation: Option<StoredPropagationPayload>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredPropagationPayload {
    node: String,
    target_cost: u32,
    bytes_hex: String,
    transient_id: [u8; 32],
    stamp_value: u32,
}
pub(in crate::bridge) fn encode_prepared(value: &PreparedDeliveryPayload) -> serde_json::Value {
    encode_prepared_parts(&value.lxmf_payload, value.propagation.as_ref())
}

pub(in crate::bridge) fn encode_prepared_parts(
    lxmf_payload: &[u8],
    propagation: Option<&PreparedPropagationPayload>,
) -> serde_json::Value {
    serde_json::to_value(StoredPreparedPayload {
        lxmf_hex: hex::encode(lxmf_payload),
        propagation: propagation.map(|p| StoredPropagationPayload {
            node: p.propagation_node_hex.clone(),
            target_cost: p.target_cost,
            bytes_hex: hex::encode(&p.payload.bytes),
            transient_id: p.payload.transient_id,
            stamp_value: p.payload.stamp_value,
        }),
    })
    .expect("serializable prepared bytes")
}
pub(in crate::bridge) fn decode_prepared(
    value: serde_json::Value,
) -> Result<PreparedDeliveryPayload, std::io::Error> {
    let value: StoredPreparedPayload =
        serde_json::from_value(value).map_err(std::io::Error::other)?;
    let propagation = value
        .propagation
        .map(|p| {
            Ok::<_, std::io::Error>(PreparedPropagationPayload {
                propagation_hash: AddressHash::new(parse_destination_hash_required(&p.node)?),
                propagation_node_hex: p.node,
                target_cost: p.target_cost,
                payload: propagation::PropagationPayload {
                    bytes: hex::decode(p.bytes_hex).map_err(std::io::Error::other)?,
                    transient_id: p.transient_id,
                    stamp_value: p.stamp_value,
                },
            })
        })
        .transpose()?;
    Ok(PreparedDeliveryPayload {
        lxmf_payload: hex::decode(value.lxmf_hex).map_err(std::io::Error::other)?,
        propagation,
    })
}
