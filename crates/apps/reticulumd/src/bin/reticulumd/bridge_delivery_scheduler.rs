use super::delivery_task::DeliveryTask;
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
    pub(super) queued_by_peer: BTreeMap<String, u64>,
    pub(super) in_flight_by_peer: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
pub(super) struct DeliverySchedulerMetrics {
    accepted_total: AtomicU64,
    rejected_queue_full_total: AtomicU64,
    queued_current: AtomicU64,
    in_flight_current: AtomicU64,
    completed_total: AtomicU64,
    peers: Mutex<HashMap<String, PeerDeliveryCounters>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct PeerDeliveryCounters {
    queued: u64,
    in_flight: u64,
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

    pub(super) fn snapshot(&self) -> DeliverySchedulerSnapshot {
        let peers = self.peers.lock().expect("delivery scheduler peer metrics mutex poisoned");
        let mut queued_by_peer = BTreeMap::new();
        let mut in_flight_by_peer = BTreeMap::new();
        for (peer, counters) in peers.iter() {
            if counters.queued > 0 {
                queued_by_peer.insert(peer.clone(), counters.queued);
            }
            if counters.in_flight > 0 {
                in_flight_by_peer.insert(peer.clone(), counters.in_flight);
            }
        }
        DeliverySchedulerSnapshot {
            accepted_total: self.accepted_total.load(Ordering::Relaxed),
            rejected_queue_full_total: self.rejected_queue_full_total.load(Ordering::Relaxed),
            queued_current: self.queued_current.load(Ordering::Relaxed),
            in_flight_current: self.in_flight_current.load(Ordering::Relaxed),
            completed_total: self.completed_total.load(Ordering::Relaxed),
            queued_by_peer,
            in_flight_by_peer,
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
            "queued_by_peer": snapshot.queued_by_peer,
            "in_flight_by_peer": snapshot.in_flight_by_peer,
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
    let mut peer_limits: HashMap<String, Arc<Semaphore>> = HashMap::new();

    while let Some(delivery) = rx.recv().await {
        let peer = delivery.task.destination_hex.clone();
        let peer_limit = peer_limits
            .entry(peer.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(config.per_peer_in_flight)))
            .clone();
        let global_limit = Arc::clone(&global_limit);
        let metrics = Arc::clone(&metrics);
        tokio::task::spawn_local(async move {
            let Ok(_global_permit) = global_limit.acquire_owned().await else {
                return;
            };
            let Ok(_peer_permit) = peer_limit.acquire_owned().await else {
                return;
            };
            metrics.record_dequeued_for_peer(&peer);
            metrics.record_started_for_peer(&peer);
            delivery.task.run().await;
            metrics.record_completed_for_peer(&peer);
        });
    }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_metrics_record_admission_and_queue_pressure() {
        let metrics = DeliverySchedulerMetrics::default();

        metrics.record_admitted_for_peer("peer-a");
        metrics.record_admitted_for_peer("peer-a");
        metrics.record_queue_full();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.accepted_total, 2);
        assert_eq!(snapshot.rejected_queue_full_total, 1);
        assert_eq!(snapshot.queued_current, 2);
    }
}
