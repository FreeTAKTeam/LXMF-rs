use super::delivery_task::{DeliveryTask, PreparedDeliveryPayload, PreparedPropagationPayload};
use super::{log_delivery_trace, propagation, RequestedDeliveryMethod};
use serde_json::{json, Value as JsonValue};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

const DEFAULT_DELIVERY_QUEUE_CAPACITY: usize = 16_384;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 32;
const DEFAULT_PER_PEER_IN_FLIGHT: usize = 1;

#[derive(Clone, Copy, Debug)]
pub(super) struct DeliverySchedulerConfig {
    pub(super) queue_capacity: usize,
    pub(super) global_concurrency: usize,
    pub(super) per_peer_in_flight: usize,
}

impl Default for DeliverySchedulerConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_DELIVERY_QUEUE_CAPACITY,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_peer_in_flight: DEFAULT_PER_PEER_IN_FLIGHT,
        }
    }
}

impl DeliverySchedulerConfig {
    pub(super) fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            queue_capacity: env_usize("LXMD_DELIVERY_QUEUE_CAPACITY")
                .unwrap_or(defaults.queue_capacity)
                .max(1),
            global_concurrency: env_usize("LXMD_DELIVERY_GLOBAL_CONCURRENCY")
                .unwrap_or(defaults.global_concurrency)
                .max(1),
            per_peer_in_flight: env_usize("LXMD_DELIVERY_PER_PEER_IN_FLIGHT")
                .unwrap_or(defaults.per_peer_in_flight)
                .max(1),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DeliverySchedulerSnapshot {
    pub(super) accepted_total: u64,
    pub(super) rejected_queue_full_total: u64,
    pub(super) queued_current: u64,
    pub(super) in_flight_current: u64,
    pub(super) completed_total: u64,
    pub(super) stamp_queued_current: u64,
    pub(super) stamp_in_flight_current: u64,
    pub(super) stamp_completed_total: u64,
    pub(super) stamp_retried_total: u64,
    pub(super) queued_by_peer: BTreeMap<String, u64>,
    pub(super) in_flight_by_peer: BTreeMap<String, u64>,
    pub(super) stamp_queued_by_peer: BTreeMap<String, u64>,
    pub(super) stamp_in_flight_by_peer: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
pub(super) struct DeliverySchedulerMetrics {
    accepted_total: AtomicU64,
    rejected_queue_full_total: AtomicU64,
    queued_current: AtomicU64,
    in_flight_current: AtomicU64,
    completed_total: AtomicU64,
    stamp_queued_current: AtomicU64,
    stamp_in_flight_current: AtomicU64,
    stamp_completed_total: AtomicU64,
    stamp_retried_total: AtomicU64,
    peers: Mutex<HashMap<String, PeerDeliveryCounters>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct PeerDeliveryCounters {
    queued: u64,
    in_flight: u64,
    stamp_queued: u64,
    stamp_in_flight: u64,
}

impl DeliverySchedulerMetrics {
    pub(super) fn record_admitted_for_peer(&self, peer: &str) {
        self.accepted_total.fetch_add(1, Ordering::Relaxed);
        self.queued_current.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| counters.queued = counters.queued.saturating_add(1));
    }

    pub(super) fn record_queue_full(&self) {
        self.rejected_queue_full_total.fetch_add(1, Ordering::Relaxed);
    }

    fn record_dequeued_for_peer(&self, peer: &str) {
        self.queued_current.fetch_sub(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| counters.queued = counters.queued.saturating_sub(1));
    }

    fn record_started_for_peer(&self, peer: &str) {
        self.in_flight_current.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.in_flight = counters.in_flight.saturating_add(1);
        });
    }

    fn record_completed_for_peer(&self, peer: &str) {
        self.in_flight_current.fetch_sub(1, Ordering::Relaxed);
        self.completed_total.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.in_flight = counters.in_flight.saturating_sub(1);
        });
    }

    fn record_finished_before_delivery_for_peer(&self, peer: &str) {
        self.queued_current.fetch_sub(1, Ordering::Relaxed);
        self.completed_total.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.queued = counters.queued.saturating_sub(1);
        });
    }

    pub(super) fn record_stamp_queued_for_peer(&self, peer: &str) {
        self.stamp_queued_current.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.stamp_queued = counters.stamp_queued.saturating_add(1);
        });
    }

    pub(super) fn record_stamp_started_for_peer(&self, peer: &str) {
        self.stamp_queued_current.fetch_sub(1, Ordering::Relaxed);
        self.stamp_in_flight_current.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.stamp_queued = counters.stamp_queued.saturating_sub(1);
            counters.stamp_in_flight = counters.stamp_in_flight.saturating_add(1);
        });
    }

    pub(super) fn record_stamp_unqueued_for_peer(&self, peer: &str) {
        self.stamp_queued_current.fetch_sub(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.stamp_queued = counters.stamp_queued.saturating_sub(1);
        });
    }

    pub(super) fn record_stamp_retry_for_peer(&self, peer: &str) {
        self.stamp_retried_total.fetch_add(1, Ordering::Relaxed);
        self.stamp_queued_current.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.stamp_queued = counters.stamp_queued.saturating_add(1);
        });
    }

    pub(super) fn record_stamp_completed_for_peer(&self, peer: &str) {
        self.stamp_in_flight_current.fetch_sub(1, Ordering::Relaxed);
        self.stamp_completed_total.fetch_add(1, Ordering::Relaxed);
        self.update_peer(peer, |counters| {
            counters.stamp_in_flight = counters.stamp_in_flight.saturating_sub(1);
        });
    }

    pub(super) fn snapshot(&self) -> DeliverySchedulerSnapshot {
        let peers = self.peers.lock().expect("delivery scheduler peer metrics mutex poisoned");
        let mut queued_by_peer = BTreeMap::new();
        let mut in_flight_by_peer = BTreeMap::new();
        let mut stamp_queued_by_peer = BTreeMap::new();
        let mut stamp_in_flight_by_peer = BTreeMap::new();
        for (peer, counters) in peers.iter() {
            if counters.queued > 0 {
                queued_by_peer.insert(peer.clone(), counters.queued);
            }
            if counters.in_flight > 0 {
                in_flight_by_peer.insert(peer.clone(), counters.in_flight);
            }
            if counters.stamp_queued > 0 {
                stamp_queued_by_peer.insert(peer.clone(), counters.stamp_queued);
            }
            if counters.stamp_in_flight > 0 {
                stamp_in_flight_by_peer.insert(peer.clone(), counters.stamp_in_flight);
            }
        }
        DeliverySchedulerSnapshot {
            accepted_total: self.accepted_total.load(Ordering::Relaxed),
            rejected_queue_full_total: self.rejected_queue_full_total.load(Ordering::Relaxed),
            queued_current: self.queued_current.load(Ordering::Relaxed),
            in_flight_current: self.in_flight_current.load(Ordering::Relaxed),
            completed_total: self.completed_total.load(Ordering::Relaxed),
            stamp_queued_current: self.stamp_queued_current.load(Ordering::Relaxed),
            stamp_in_flight_current: self.stamp_in_flight_current.load(Ordering::Relaxed),
            stamp_completed_total: self.stamp_completed_total.load(Ordering::Relaxed),
            stamp_retried_total: self.stamp_retried_total.load(Ordering::Relaxed),
            queued_by_peer,
            in_flight_by_peer,
            stamp_queued_by_peer,
            stamp_in_flight_by_peer,
        }
    }

    fn update_peer(&self, peer: &str, update: impl FnOnce(&mut PeerDeliveryCounters)) {
        let mut peers = self.peers.lock().expect("delivery scheduler peer metrics mutex poisoned");
        let counters = peers.entry(peer.to_string()).or_default();
        update(counters);
    }
}

#[derive(Clone)]
pub(super) struct DeliveryScheduler {
    config: DeliverySchedulerConfig,
    tx: mpsc::Sender<ScheduledDelivery>,
    backlog_limit: Arc<Semaphore>,
    metrics: Arc<DeliverySchedulerMetrics>,
}

impl DeliveryScheduler {
    pub(super) fn spawn(config: DeliverySchedulerConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.queue_capacity);
        let backlog_limit = Arc::new(Semaphore::new(config.queue_capacity));
        let metrics = Arc::new(DeliverySchedulerMetrics::default());
        let runtime_metrics = Arc::clone(&metrics);
        std::thread::Builder::new()
            .name("rpc-outbound-delivery-runtime".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build outbound delivery runtime");
                let local = tokio::task::LocalSet::new();
                local.block_on(&runtime, run_scheduler(rx, config, runtime_metrics));
            })
            .expect("spawn rpc outbound delivery runtime");

        Self { config, tx, backlog_limit, metrics }
    }

    pub(super) fn enqueue(&self, task: DeliveryTask) -> Result<(), std::io::Error> {
        let peer = task.destination_hex.clone();
        let capacity_permit = self.backlog_limit.clone().try_acquire_owned().map_err(|_| {
            self.metrics.record_queue_full();
            std::io::Error::new(std::io::ErrorKind::WouldBlock, "outbound delivery queue full")
        })?;
        match self.tx.try_send(ScheduledDelivery { task, _capacity_permit: capacity_permit }) {
            Ok(()) => {
                self.metrics.record_admitted_for_peer(&peer);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.metrics.record_queue_full();
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "outbound delivery queue full",
                ))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "outbound delivery runtime stopped",
            )),
        }
    }

    pub(super) fn status_json(&self) -> JsonValue {
        let snapshot = self.metrics.snapshot();
        json!({
            "queue_capacity": self.config.queue_capacity,
            "global_concurrency": self.config.global_concurrency,
            "per_peer_in_flight": self.config.per_peer_in_flight,
            "accepted_total": snapshot.accepted_total,
            "rejected_queue_full_total": snapshot.rejected_queue_full_total,
            "queued_total": snapshot.queued_current,
            "in_flight_total": snapshot.in_flight_current,
            "completed_total": snapshot.completed_total,
            "stamp_queued_total": snapshot.stamp_queued_current,
            "stamp_in_flight_total": snapshot.stamp_in_flight_current,
            "stamp_completed_total": snapshot.stamp_completed_total,
            "stamp_retried_total": snapshot.stamp_retried_total,
            "queued_by_peer": snapshot.queued_by_peer,
            "in_flight_by_peer": snapshot.in_flight_by_peer,
            "stamp_queued_by_peer": snapshot.stamp_queued_by_peer,
            "stamp_in_flight_by_peer": snapshot.stamp_in_flight_by_peer,
        })
    }
}

struct ScheduledDelivery {
    task: DeliveryTask,
    _capacity_permit: OwnedSemaphorePermit,
}

async fn run_scheduler(
    mut rx: mpsc::Receiver<ScheduledDelivery>,
    config: DeliverySchedulerConfig,
    metrics: Arc<DeliverySchedulerMetrics>,
) {
    let global_limit = Arc::new(Semaphore::new(config.global_concurrency));
    let stamp_limit = Arc::new(Semaphore::new(1));
    let mut peer_limits: HashMap<String, Arc<Semaphore>> = HashMap::new();

    while let Some(delivery) = rx.recv().await {
        let peer = delivery.task.destination_hex.clone();
        if delivery.task.requires_deferred_stamp_work() {
            delivery.task.record_deferred_stamp_queued_metadata();
            metrics.record_stamp_queued_for_peer(&peer);
        }
        let peer_limit = peer_limits
            .entry(peer.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(config.per_peer_in_flight)))
            .clone();
        let global_limit = Arc::clone(&global_limit);
        let stamp_limit = Arc::clone(&stamp_limit);
        let metrics = Arc::clone(&metrics);
        tokio::task::spawn_local(async move {
            let prepared = prepare_payload(&delivery.task, &stamp_limit, &metrics, &peer).await;
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
            delivery.task.run_prepared(prepared, stamp_limit).await;
            metrics.record_completed_for_peer(&peer);
        });
    }
}

async fn prepare_payload(
    task: &DeliveryTask,
    stamp_limit: &Arc<Semaphore>,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
) -> Option<PreparedDeliveryPayload> {
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
    Some(PreparedDeliveryPayload { lxmf_payload, propagation })
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
        let _stamp_permit = stamp_limit.clone().acquire_owned().await.ok()?;
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
            || {
                let status = task.daemon.message_receipt_status(&task.message_id).ok().flatten();
                DeliveryTask::is_cancelled_status(status.as_deref())
            },
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

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<usize>().ok())
}

#[cfg(test)]
#[path = "bridge_delivery_scheduler_tests.rs"]
mod tests;
