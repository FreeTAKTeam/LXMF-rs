//! Pinned production-router oracle versus the production admitted Rust owner.
//! Measurements/turn scheduling and Error/NoReceipt producer faults are controlled
//! exactly as in the Python oracle. Native Links, authenticated ingress, packet
//! proofs, Resources, SQLite commits and admission are real. This is not mixed-peer
//! or native watchdog timing evidence; those are separate acceptance lanes.
use super::*;
use crate::bridge::delivery_task::PreparedPropagationPayload;
use crate::bridge::delivery_wakeup::trace_control::{Observation, ProducerFault, TraceControl};
use reticulum_daemon::receipt_bridge::{ReceiptBridge, ReceiptPublisher};
use rns_rpc::BrokerCommand;
use rns_transport::destination::{DestinationDesc, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::identity::PrivateIdentity as NativeIdentity;
use rns_transport::iface::{IfaceRole, InterfaceChannel};
use rns_transport::packet::{Packet, PacketContext};
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::Value;
use std::sync::atomic::Ordering as AtomicOrdering;
use tokio::time::Instant;

#[path = "bridge_delivery_link_trace_admission_tests.rs"]
mod admission;
#[path = "bridge_delivery_link_trace_assert_tests.rs"]
mod assertions;
#[path = "bridge_delivery_link_trace_wire_tests.rs"]
mod wire;

struct MessageOwner {
    id: String,
    control: Arc<TraceControl>,
    wakeup: Arc<crate::bridge::delivery_wakeup::DeliveryWakeup>,
    owner: tokio::task::JoinHandle<()>,
    admitted_at: Instant,
    expected: Vec<u8>,
}
struct Harness {
    _directory: tempfile::TempDir,
    connection: rusqlite::Connection,
    admissions: MessagesStore,
    daemon: Arc<RpcDaemon>,
    publisher: ReceiptPublisher,
    receipt_rx: Option<tokio::sync::mpsc::Receiver<reticulum_daemon::receipt_bridge::ReceiptEvent>>,
    receipt_owner: Option<crate::receipt_worker::ReceiptOwner>,
    scheduler: DeliveryScheduler,
    global: Arc<Semaphore>,
    stamp: Arc<Semaphore>,
    stop: tokio::sync::watch::Sender<bool>,
    clock_guard: tokio::task::JoinHandle<()>,
    messages: Vec<MessageOwner>,
    wire: wire::Wire,
    propagation_link:
        Arc<tokio::sync::Mutex<Option<crate::bridge::propagation::CachedPropagationLink>>>,
    receipt_map: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    resources: crate::outbound_resources::OutboundResourceMap,
    origin: Instant,
    propagated: bool,
    method: RequestedDeliveryMethod,
    path_known: bool,
    measurements: (f64, u64, f64),
}
fn identity(byte: u8) -> NativeIdentity {
    NativeIdentity::from_private_key_bytes(&[byte; 64]).expect("reference identity")
}

#[tokio::test(start_paused = true)]
async fn retained_owner_consumes_all_pinned_python_link_trace_rows() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../libs/test-support/fixtures/lxmf_1_2_0_link_traces.json"
    )))
    .expect("oracle");
    assert_eq!(fixture["lxmf_commit"], "c3ff2d6dc2f256daab896dadc044dd5a913ecbb7");
    assert_eq!(fixture["rns_commit"], "99de23c040d507e3fefca19e87b182302902725d");
    let cases = fixture["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 47);
    let mut rows = 0;
    for case in cases {
        eprintln!("reference owner case {}", case["name"]);
        let mut harness = Harness::new(case).await;
        for (index, row) in case["rows"].as_array().expect("rows").iter().enumerate() {
            harness.apply(case, row, index).await;
            assertions::assert_row(&mut harness, case, row, index).await;
            rows += 1;
        }
        harness.shutdown().await;
    }
    assert_eq!(rows, 263, "every covered snapshot was consumed");
}

impl Harness {
    async fn new(case: &Value) -> Self {
        let propagated = case["requested_method"] == "propagated";
        let method = if propagated {
            RequestedDeliveryMethod::Propagated
        } else if case["requested_method"] == "opportunistic" {
            RequestedDeliveryMethod::Opportunistic
        } else {
            RequestedDeliveryMethod::Direct
        };
        let recipient =
            SingleInputDestination::new(identity(0x22), DestinationName::new("lxmf", "delivery"));
        let (_directory, connection, task) = durable_task_for_destination(
            recipient.desc.address_hash.as_slice().try_into().expect("hash"),
        );
        connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy store");
        let admissions = MessagesStore::open(&_directory.path().join("delivery.db"))
            .expect("admission connection");
        let daemon = task.daemon.clone();
        let wire = wire::Wire::new(task.transport.clone(), propagated).await;
        let (sender, receipt_rx) = tokio::sync::mpsc::channel(16);
        let publisher = ReceiptPublisher::from(sender);
        let (stop, _) = tokio::sync::watch::channel(false);
        let clock_stop = stop.subscribe();
        let clock_guard = tokio::spawn(async move {
            while !*clock_stop.borrow() {
                tokio::task::yield_now().await;
            }
        });
        let mut harness = Self {
            _directory,
            connection,
            admissions,
            daemon,
            publisher,
            receipt_rx: Some(receipt_rx),
            receipt_owner: None,
            scheduler: DeliveryScheduler::spawn(DeliverySchedulerConfig::default()),
            global: Arc::new(Semaphore::new(1)),
            stamp: Arc::new(Semaphore::new(1)),
            stop,
            clock_guard,
            messages: Vec::new(),
            propagation_link: task.outbound_propagation_link,
            receipt_map: task.receipt_map,
            resources: task.outbound_resource_map,
            origin: Instant::now(),
            propagated,
            method,
            wire,
            path_known: case["initial_path_known"].as_bool().expect("path input"),
            measurements: (
                case["medium_timeout"].as_f64().expect("medium input"),
                case["lowest_bitrate"].as_u64().expect("bitrate input"),
                case["first_hop"].as_f64().expect("first input"),
            ),
        };
        harness
            .wire
            .sender
            .set_receipt_handler(Box::new(ReceiptBridge::new(
                harness.receipt_map.clone(),
                harness.publisher.clone(),
            )))
            .await;
        if case["initial_path_known"].as_bool().expect("initial path") {
            harness.wire.set_path(true).await;
        }
        harness.wire.initial_links(case["initial_links"].as_str(), &harness.propagation_link).await;
        harness.admit(case, 0, Some(task.message_id)).await;
        if case["initial_messages"] == 2 {
            harness.admit(case, 1, None).await;
        }
        harness
    }
    fn start_receipts(&mut self) {
        if let Some(rx) = self.receipt_rx.take() {
            self.receipt_owner = Some(crate::receipt_worker::spawn_receipt_worker(
                self.daemon.clone(),
                rx,
                self.receipt_map.clone(),
                self.publisher.clone(),
            ));
        }
    }
    async fn apply(&mut self, case: &Value, row: &Value, index: usize) {
        let event = &row["event"];
        for key in event.as_object().expect("event object").keys() {
            assert!(
                [
                    "admit",
                    "at",
                    "path",
                    "medium",
                    "bitrate",
                    "first",
                    "activate",
                    "close",
                    "proof",
                    "timeout",
                    "resource",
                    "cancel",
                    "process",
                    "signal",
                    "signal_link",
                    "admit_new"
                ]
                .contains(&key.as_str()),
                "unsupported oracle input {key}"
            );
        }
        let at =
            Duration::from_secs_f64(event["at"].as_f64().map_or(0.0, |at| (1000.0 + at) - 1000.0));
        let target = self.origin + at;
        assert!(target >= Instant::now());
        tokio::time::advance(target - Instant::now()).await;
        if let Some(known) = event["path"].as_bool() {
            self.path_known = known;
        }
        if let Some(medium) = event["medium"].as_f64() {
            self.measurements.0 = medium;
        }
        if let Some(bitrate) = event["bitrate"].as_u64() {
            self.measurements.1 = bitrate;
        }
        if let Some(first) = event["first"].as_f64() {
            self.measurements.2 = first;
        }
        if event["admit_new"] == true {
            self.admit(case, 2, None).await;
        }
        for message in &self.messages {
            *message.control.measurements.lock().expect("measurements") = self.measurements;
        }
        if event["activate"] == true {
            self.wire.activate_latest().await;
        }
        if event["close"] == true {
            self.wire.close_latest().await;
        }
        if event["cancel"] == true {
            // The oracle assigns state directly. The Rust projection goes through
            // its real receipt transaction and retains monotonic terminal state.
            self.start_receipts();
            self.publisher
                .try_send(reticulum_daemon::receipt_bridge::ReceiptEvent::new(
                    &self.messages[0].id,
                    "cancelled",
                ))
                .expect("cancel receipt");
            for _ in 0..10000 {
                if self
                    .daemon
                    .message_receipt_status(&self.messages[0].id)
                    .expect("status")
                    .as_deref()
                    == Some("cancelled")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert_eq!(
                self.daemon
                    .message_receipt_status(&self.messages[0].id)
                    .expect("status")
                    .as_deref(),
                Some("cancelled")
            );
        }
        if let Some(receipt) = event["timeout"].as_u64() {
            let token = self.packet_tokens()[receipt as usize].clone();
            for message in &self.messages {
                message
                    .control
                    .expire
                    .lock()
                    .expect("expire input")
                    .insert(token.packet_hash().to_bytes());
            }
        }
        if let Some(proof) = event["proof"].as_u64() {
            self.wire.prove(proof as usize).await;
            let receipt = self.packet_tokens()[proof as usize].clone();
            for _ in 0..10000 {
                if receipt.is_proved() {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert!(receipt.is_proved(), "native proof input must finish before its snapshot");
        }
        if let Some(outcome) = event["resource"].as_str() {
            if event["cancel"] == true {
                // Explicit monotonic-terminal projection: the reference invokes
                // a synthetic callback after assigning CANCELLED. Drive Rust's
                // committed cancellation and assert the real native result is
                // immutable; never fabricate a second Resource outcome.
                self.turn(0).await;
                let observation = self.messages[0]
                    .control
                    .observation
                    .lock()
                    .expect("cancelled Resource")
                    .clone();
                assert_eq!(
                    observation.resource_outcome,
                    Some(rns_transport::resource::ResourceOutcome::Cancelled)
                );
                assert!(!self
                    .resources
                    .lock()
                    .expect("Resource cleanup")
                    .contains_key(&observation.resource.expect("native resource").to_string()));
                assert_eq!(outcome, "failed", "only the declared post-cancel failure projection");
            } else {
                self.wire.resource(outcome).await;
            }
        }
        if let Some(signal) = event["signal"].as_str() {
            self.wire
                .signal(
                    event["signal_link"].as_u64().unwrap_or(0) as usize,
                    &hex::decode(signal).expect("signal bytes"),
                )
                .await;
        }
        if event["process"] != false {
            // Native pending-Link maintenance may expire a real path. The oracle
            // controls has_path independently of watchdog effects; restore that
            // declared input after native cleanup, using signed announcements.
            self.wire.pump().await;
            self.wire.set_path(self.path_known).await;
            if event.get("proof").is_some()
                || event.get("resource").is_some()
                || event.get("signal").is_some()
            {
                self.start_receipts();
            }
            // Each call to the pinned router gives each pending message one turn.
            // Internal acquisition/closure/dispatch decisions are observed at their
            // next loop boundary, without letting autonomous timers run ahead.
            for item in 0..self.messages.len() {
                self.turn(item).await;
            }
        }
        self.wire.pump().await;
        if self.receipt_owner.is_some() {
            // Keep the oracle clock paused while the real receipt worker commits.
            let started = std::time::Instant::now();
            while self.messages.iter().any(|m| {
                self.publisher.terminal_pending(&m.id).expect("terminal fence")
                    || (m.control.done.load(AtomicOrdering::Acquire) && !m.owner.is_finished())
            }) {
                assert!(
                    started.elapsed() < Duration::from_secs(2),
                    "native completion must commit and retire before its snapshot"
                );
                tokio::task::yield_now().await;
            }
        }
        if index == 0 {
            assert_eq!(self.global.available_permits(), 1);
        }
    }
    fn packet_tokens(&self) -> Vec<rns_transport::transport::LinkPacketReceipt> {
        let mut tokens = self
            .messages
            .iter()
            .flat_map(|m| m.control.observation.lock().expect("observation").packets.clone())
            .collect::<Vec<_>>();
        tokens.sort_by_key(|token| token.sent_at());
        tokens
    }
    async fn turn(&mut self, index: usize) {
        let control = self.messages[index].control.clone();
        if control.done.load(AtomicOrdering::Acquire) {
            return;
        }
        let before = control.completed.load(AtomicOrdering::Acquire);
        control.grant();
        self.messages[index].wakeup.changed.notify_one();
        for _ in 0..20000 {
            self.wire.pump().await;
            if self
                .messages
                .iter()
                .any(|m| self.publisher.terminal_pending(&m.id).expect("terminal publication"))
            {
                self.start_receipts();
            }
            if control.completed.load(AtomicOrdering::Acquire) > before
                || control.done.load(AtomicOrdering::Acquire)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "owner did not finish reference decision message={} attempts={}",
            self.messages[index].id,
            control.observation.lock().expect("observation").attempts
        );
    }
    async fn shutdown(self) {
        self.stop.send_replace(true);
        self.clock_guard.await.expect("manual clock guard");
        for message in self.messages {
            message.owner.await.expect("native owner shutdown");
        }
        if let Some(owner) = self.receipt_owner {
            owner.drain().await.expect("receipt drain");
        }
        assert!(self.scheduler.owned_messages.lock().expect("ownership").is_empty());
        self.scheduler.shutdown();
    }
}
