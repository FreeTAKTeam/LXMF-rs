use super::{DeliverySchedulerMetrics, DeliveryTask};
use crate::bridge::delivery_task::{
    emit_receipt_event, PreparedDeliveryPayload, PreparedPropagationPayload,
};
use crate::bridge::{log_delivery_trace, propagation, RequestedDeliveryMethod};
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
        if task.requires_deferred_stamp_work() {
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
            if let Err(error) = task.daemon.defer_durable_dispatch(&task.message_id) {
                log::error!("prepared payload lookup recovery could not be persisted: {error}");
            }
            return None;
        }
    };
    task.start_delivery_trace();
    if task.abort_if_cancelled("start") {
        task.record_deferred_stamp_cancelled_metadata();
        if task.requires_deferred_stamp_work() {
            metrics.record_stamp_started_for_peer(peer);
            metrics.record_stamp_completed_for_peer(peer);
        }
        return None;
    }

    let lxmf_payload = prepare_lxmf_payload(task, stamp_limit, metrics, peer).await?;
    let propagation = if task.requested_method == RequestedDeliveryMethod::Propagated {
        if task.requires_normal_deferred_stamp_work() {
            metrics.record_stamp_queued_for_peer(peer);
        }
        Some(prepare_propagation_payload(task, &lxmf_payload, stamp_limit, metrics, peer).await?)
    } else {
        None
    };
    let prepared = PreparedDeliveryPayload { lxmf_payload, propagation };
    if durable {
        if let Err(error) = task
            .daemon
            .persist_durable_prepared_payload(&task.message_id, encode_prepared(&prepared))
        {
            log::error!("prepared wire payload was not committed; network send suppressed message_id={}: {error}",task.message_id);
            if let Err(error) = task.daemon.defer_durable_dispatch(&task.message_id) {
                log::error!("durable dispatch could not be deferred: {error}");
            }
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
            let permit = stamp_limit.clone().acquire_owned().await.ok()?;
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

async fn prepare_propagation_payload(
    task: &DeliveryTask,
    lxmf_payload: &[u8],
    stamp_limit: &Arc<Semaphore>,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
) -> Option<PreparedPropagationPayload> {
    let Some(context) = task.propagation_preparation_context().await else {
        metrics.record_stamp_unqueued_for_peer(peer);
        return None;
    };
    log_delivery_trace(
        &task.message_id,
        &task.destination_hex,
        "propagation",
        "building propagation payload",
    );
    task.record_propagation_stamp_work_metadata("queued", context.target_cost, None);
    for attempt in 1..=2u32 {
        let _stamp_permit = match stamp_limit.clone().acquire_owned().await {
            Ok(permit) => permit,
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
                emit_receipt_event(
                    &task.receipt_tx,
                    ReceiptEvent::new(task.message_id.clone(), "failed: stamp worker stopped")
                        .with_method("propagation"),
                );
                return None;
            }
        };
        metrics.record_stamp_started_for_peer(peer);
        task.record_propagation_stamp_attempt_metadata(context.target_cost, attempt);
        if task.abort_if_cancelled("propagation") {
            task.record_propagation_stamp_work_metadata("cancelled", context.target_cost, None);
            metrics.record_stamp_completed_for_peer(peer);
            return None;
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
                return Some(PreparedPropagationPayload {
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
                    return None;
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
                return None;
            }
        }
    }
    None
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
    serde_json::to_value(StoredPreparedPayload {
        lxmf_hex: hex::encode(&value.lxmf_payload),
        propagation: value.propagation.as_ref().map(|p| StoredPropagationPayload {
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
