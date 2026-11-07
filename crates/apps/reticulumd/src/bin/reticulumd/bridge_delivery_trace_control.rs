//! Test-only clock/producer inputs and passive observations for the pinned oracle.
//! The fixture drives the production admitted owner and native Transport. This
//! controls decision turns/measurements and explicit producer faults, not policy.
use lxmf::message::{DeliverySchedule, DeliveryTiming};
use rns_transport::destination::link::Link;
use rns_transport::transport::LinkPacketReceipt;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{watch, Semaphore};
use tokio::time::Instant;

#[derive(Clone, Default)]
pub(in crate::bridge) struct Observation {
    pub attempts: u32,
    pub schedule: DeliverySchedule,
    pub never_active_requested: bool,
    pub origin: Option<Instant>,
    pub link: Option<Arc<tokio::sync::Mutex<Link>>>,
    pub creator: bool,
    pub sending: bool,
    pub packets: Vec<LinkPacketReceipt>,
    pub resource: Option<rns_transport::hash::Hash>,
    pub resource_outcome: Option<rns_transport::resource::ResourceOutcome>,
}
#[derive(Clone)]
pub(in crate::bridge) struct DispatchObservation {
    pub link: Arc<tokio::sync::Mutex<Link>>,
    pub bytes: Vec<u8>,
    pub resource: bool,
}
#[derive(Clone, Copy, Default)]
pub(in crate::bridge) enum ProducerFault {
    #[default]
    None,
    Error,
    NoReceipt,
}
pub(in crate::bridge) struct TraceControl {
    turns: Semaphore,
    active: AtomicBool,
    pub completed: AtomicU64,
    pub done: AtomicBool,
    pub observation: Mutex<Observation>,
    pub dispatches: Mutex<Vec<DispatchObservation>>,
    pub requests: Mutex<Vec<Instant>>,
    pub teardowns: Mutex<Vec<Arc<tokio::sync::Mutex<Link>>>>,
    pub measurements: Mutex<(f64, u64, f64)>,
    pub expire: Mutex<HashSet<[u8; 32]>>,
    pub fault: Mutex<ProducerFault>,
}
impl Default for TraceControl {
    fn default() -> Self {
        Self {
            turns: Semaphore::new(0),
            active: AtomicBool::new(false),
            completed: AtomicU64::new(0),
            done: AtomicBool::new(false),
            observation: Mutex::new(Observation::default()),
            dispatches: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
            teardowns: Mutex::new(Vec::new()),
            measurements: Mutex::new((8.0, 2000, 6.0)),
            expire: Mutex::new(HashSet::new()),
            fault: Mutex::new(ProducerFault::None),
        }
    }
}
impl TraceControl {
    pub async fn enter(
        &self,
        observation: Observation,
        mut stopping: watch::Receiver<bool>,
    ) -> bool {
        self.observe(observation);
        self.finish_turn();
        tokio::select! {
            permit = self.turns.acquire() => {
                permit.expect("trace gate stays open").forget();
                self.active.store(true, Ordering::Release);
                true
            },
            _ = async { while !*stopping.borrow() { if stopping.changed().await.is_err() { break; } } } => false,
        }
    }
    pub fn observe(&self, value: Observation) {
        *self.observation.lock().expect("trace observation") = value;
    }
    pub fn finish_turn(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            self.completed.fetch_add(1, Ordering::Release);
        }
    }
    pub fn finish(&self) {
        self.finish_turn();
        self.done.store(true, Ordering::Release);
    }
    pub fn grant(&self) {
        self.turns.add_permits(1);
    }
    pub fn timing(&self, path_known: bool) -> DeliveryTiming {
        let (medium, bitrate, first) = *self.measurements.lock().expect("trace measurements");
        let rtt = if path_known { 2.0 * (first - 6.0).max(0.0) + 6.0 } else { medium };
        DeliveryTiming::from_round_trip(medium, Some(bitrate), rtt).expect("reference measurements")
    }
    pub fn expired(&self, receipt: &LinkPacketReceipt) -> bool {
        // The oracle disables RNS's autonomous timer and invokes timeout callbacks
        // explicitly. Keep the real native expiry predicate, with a per-token now.
        let sent = receipt.sent_at().unwrap_or_else(std::time::Instant::now);
        let expire = self
            .expire
            .lock()
            .expect("trace timeout inputs")
            .contains(&receipt.packet_hash().to_bytes());
        receipt.expired(if expire {
            sent + receipt.timeout() + Duration::from_nanos(1)
        } else {
            sent
        })
    }
}
