use super::*;
use crate::bridge::delivery_task::PreparedDeliveryPayload;

/// Exercise the production prepared-message driver. Admission and commit fences
/// are covered separately through complete_delivery and the durable broker.
pub(in crate::bridge) fn start_prepared(
    mut task: DeliveryTask,
    mut prepared: PreparedDeliveryPayload,
) -> (tokio::sync::watch::Sender<bool>, tokio::task::JoinHandle<()>) {
    let (stop, mut stopping) = tokio::sync::watch::channel(false);
    task.stopping = Some(stopping.clone());
    let metrics = Arc::new(DeliverySchedulerMetrics::default());
    metrics.record_admitted_for_peer(&task.destination_hex);
    let owner = tokio::spawn(async move {
        let peer = task.destination_hex.clone();
        completion::run_prepared(
            &mut task,
            &mut prepared,
            DeliveryLimits {
                global: Arc::new(Semaphore::new(1)),
                peer: Arc::new(Semaphore::new(1)),
                stamp: Arc::new(Semaphore::new(1)),
            },
            metrics,
            &peer,
            &mut stopping,
        )
        .await;
    });
    (stop, owner)
}
