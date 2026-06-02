use crate::{
    ble::{BleShimConfig, BleShimTransport},
    constants::DEFAULT_CAPTURE_MAX_BYTES,
    RuntimeConfig, RuntimeEvent, RuntimeStats,
};
use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    vec::Vec,
};
use rns_embedded_core::{store::JournaledEmbeddedStore, transport::LinkState, EmbeddedError};

#[cfg(feature = "std")]
use alloc::sync::Arc;

#[cfg(not(feature = "std"))]
use alloc::rc::Rc;

#[cfg(not(feature = "std"))]
use core::cell::RefCell;

#[cfg(feature = "std")]
use std::{
    net::{SocketAddr, TcpListener, ToSocketAddrs},
    sync::{Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(feature = "std")]
use crate::tcp::TcpEmbeddedTransport;

#[cfg(feature = "std")]
const DRIVER_TICK_MS: u64 = 25;
#[cfg(feature = "std")]
const DRIVER_TICK_SLEEP: Duration = Duration::from_millis(DRIVER_TICK_MS);
// FFI callers can pass arbitrarily large timeouts; clamp the blocking surface to a
// practical upper bound so impossible waits degrade into a safe timeout result.
#[cfg(feature = "std")]
const MAX_BLOCKING_TIMEOUT_MS: u64 = u32::MAX as u64;
pub const NODE_EXTENSION_ID_BOOTSTRAPPED: u32 = 1;
pub const NODE_EXTENSION_ID_MESSAGE_QUEUED: u32 = 2;
pub const NODE_EXTENSION_ID_RECEIVED_SUMMARY: u32 = 3;
#[cfg(feature = "std")]
const DEFAULT_TCP_MTU_HINT: u16 = 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeTransportMode {
    BleOnly,
    TcpClient,
    TcpServer,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeLifecycleState {
    Boot,
    Unprovisioned,
    ProvisionedOffline,
    TcpOnline,
    BleRecovery,
    FailureReconnect,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CaptureDefaults {
    pub max_bytes: u32,
}

impl Default for CaptureDefaults {
    fn default() -> Self {
        Self { max_bytes: DEFAULT_CAPTURE_MAX_BYTES }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BleNodeBackendConfig {
    pub mtu_hint: u16,
    pub max_inbound_frames: usize,
    pub max_outbound_frames: usize,
    pub ordered_delivery: bool,
}

impl Default for BleNodeBackendConfig {
    fn default() -> Self {
        let config = BleShimConfig::default();
        Self {
            mtu_hint: config.mtu_hint,
            max_inbound_frames: config.max_inbound_frames,
            max_outbound_frames: config.max_outbound_frames,
            ordered_delivery: config.ordered_delivery,
        }
    }
}

impl From<&BleNodeBackendConfig> for BleShimConfig {
    fn from(value: &BleNodeBackendConfig) -> Self {
        Self {
            mtu_hint: value.mtu_hint,
            max_inbound_frames: value.max_inbound_frames,
            max_outbound_frames: value.max_outbound_frames,
            ordered_delivery: value.ordered_delivery,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TcpClientConfig {
    pub host: String,
    pub port: u16,
    pub reconnect_backoff_ms: Vec<u64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TcpServerConfig {
    pub listen_port: u16,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NodeBackendConfig {
    Ble(BleNodeBackendConfig),
    #[cfg(feature = "std")]
    TcpClient(TcpClientConfig),
    #[cfg(feature = "std")]
    TcpServer(TcpServerConfig),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NodeConfig {
    pub runtime: RuntimeConfig,
    pub backend: NodeBackendConfig,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            runtime: RuntimeConfig::default(),
            backend: NodeBackendConfig::Ble(BleNodeBackendConfig::default()),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeRunState {
    Stopped,
    Running,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeLogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeOperationKind {
    Send,
    Broadcast,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct NodeOperationReceipt {
    pub operation: NodeOperationKind,
    pub operation_id: u64,
    pub epoch: u64,
    pub accepted_bytes: usize,
    pub queued: bool,
    pub target_count: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SendOptions;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct BroadcastOptions {
    pub destinations: Vec<[u8; 16]>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct NodeStatus {
    pub run_state: NodeRunState,
    pub epoch: u64,
    pub lifecycle_state: Option<NodeLifecycleState>,
    pub pending_outbound: usize,
    pub stats: RuntimeStats,
    pub log_level: NodeLogLevel,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NodeError {
    InvalidConfig,
    IoError,
    NetworkError,
    ReticulumError,
    AlreadyRunning,
    NotRunning,
    Timeout,
    InternalError,
    ModeConflict,
    SubscriptionClosed,
    NodeRestarted,
    EventGap,
    QueuePressure,
}

impl From<EmbeddedError> for NodeError {
    fn from(value: EmbeddedError) -> Self {
        match value {
            EmbeddedError::InvalidInput
            | EmbeddedError::InvalidArgument
            | EmbeddedError::Unsupported => Self::InvalidConfig,
            EmbeddedError::Timeout => Self::Timeout,
            EmbeddedError::Backpressure => Self::QueuePressure,
            EmbeddedError::Disconnected => Self::NetworkError,
            EmbeddedError::IntegrityFailure
            | EmbeddedError::ChecksumMismatch
            | EmbeddedError::IdempotencyConflict
            | EmbeddedError::ReplayRejected
            | EmbeddedError::SeqGap
            | EmbeddedError::NotFound
            | EmbeddedError::InvalidCursor => Self::ReticulumError,
            EmbeddedError::StorageCorruption | EmbeddedError::InvalidState => Self::InternalError,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NodeEventKind {
    StatusChanged { run_state: NodeRunState, lifecycle_state: Option<NodeLifecycleState> },
    Log { level: NodeLogLevel, code: u32 },
    Error { error: NodeError, frame_kind: u8, sequence: u32 },
    PacketReceived { frame_kind: u8, sequence: u32, bytes: usize },
    PacketSent { frame_kind: u8, sequence: u32, bytes: usize },
    Extension { extension_id: u32, value0: u64, value1: u64 },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NodeEvent {
    pub event_id: u64,
    pub epoch: u64,
    pub occurred_at_ms: u64,
    pub operation_id: Option<u64>,
    pub kind: NodeEventKind,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PollResult {
    Event(NodeEvent),
    Timeout,
    Closed,
    Gap { next_event_id: u64 },
    NodeStopped,
    NodeRestarted { epoch: u64 },
}

enum NodeBackend {
    Ble(BleShimTransport),
    #[cfg(feature = "std")]
    Tcp(TcpEmbeddedTransport),
}

impl NodeBackend {
    fn set_link_state(&mut self, state: LinkState) {
        match self {
            Self::Ble(transport) => transport.set_link_state(state),
            #[cfg(feature = "std")]
            Self::Tcp(_) => {}
        }
    }

    fn push_inbound_wire(&mut self, bytes: &[u8]) -> Result<(), NodeError> {
        match self {
            Self::Ble(transport) => transport.push_inbound_wire(bytes).map_err(NodeError::from),
            #[cfg(feature = "std")]
            Self::Tcp(_) => {
                let _ = bytes;
                Err(NodeError::ModeConflict)
            }
        }
    }

    fn take_outbound_wire(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Ble(transport) => transport.take_outbound_wire(),
            #[cfg(feature = "std")]
            Self::Tcp(_) => None,
        }
    }

    fn link_state(&self) -> LinkState {
        match self {
            Self::Ble(transport) => {
                rns_embedded_core::transport::EmbeddedTransport::link_state(transport)
            }
            #[cfg(feature = "std")]
            Self::Tcp(transport) => {
                rns_embedded_core::transport::EmbeddedTransport::link_state(transport)
            }
        }
    }
}

#[cfg(feature = "std")]
fn resolve_tcp_addr(config: &TcpClientConfig) -> Result<SocketAddr, NodeError> {
    (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|_| NodeError::InvalidConfig)?
        .next()
        .ok_or(NodeError::InvalidConfig)
}

#[cfg(feature = "std")]
fn tcp_server_transport(config: &TcpServerConfig) -> Result<TcpEmbeddedTransport, NodeError> {
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.listen_port)))
        .map_err(|_| NodeError::IoError)?;
    let (stream, _) = listener.accept().map_err(|_| NodeError::IoError)?;
    TcpEmbeddedTransport::from_stream(stream, DEFAULT_TCP_MTU_HINT).map_err(NodeError::from)
}

struct RuntimeSession {
    epoch: u64,
    runtime: crate::EmbeddedNodeRuntime,
    store: JournaledEmbeddedStore,
    backend: NodeBackend,
}

impl RuntimeSession {
    fn new(epoch: u64, config: &NodeConfig) -> Result<Self, NodeError> {
        let runtime = crate::EmbeddedNodeRuntime::new(config.runtime).map_err(NodeError::from)?;
        let backend = match &config.backend {
            NodeBackendConfig::Ble(ble) => {
                if config.runtime.node_mode != NodeTransportMode::BleOnly {
                    return Err(NodeError::InvalidConfig);
                }
                NodeBackend::Ble(
                    BleShimTransport::new(BleShimConfig::from(ble)).map_err(NodeError::from)?,
                )
            }
            #[cfg(feature = "std")]
            NodeBackendConfig::TcpClient(tcp) => {
                if config.runtime.node_mode != NodeTransportMode::TcpClient {
                    return Err(NodeError::InvalidConfig);
                }
                let addr = resolve_tcp_addr(tcp)?;
                NodeBackend::Tcp(
                    TcpEmbeddedTransport::connect(addr, DEFAULT_TCP_MTU_HINT)
                        .map_err(NodeError::from)?,
                )
            }
            #[cfg(feature = "std")]
            NodeBackendConfig::TcpServer(tcp) => {
                if config.runtime.node_mode != NodeTransportMode::TcpServer {
                    return Err(NodeError::InvalidConfig);
                }
                NodeBackend::Tcp(tcp_server_transport(tcp)?)
            }
        };

        Ok(Self { epoch, runtime, store: JournaledEmbeddedStore::new(), backend })
    }

    fn tick(&mut self, now_ms: u64) -> Result<Vec<RuntimeEvent>, NodeError> {
        match &mut self.backend {
            NodeBackend::Ble(transport) => {
                self.runtime.tick(now_ms, transport, &mut self.store).map_err(NodeError::from)?
            }
            #[cfg(feature = "std")]
            NodeBackend::Tcp(transport) => {
                self.runtime.tick(now_ms, transport, &mut self.store).map_err(NodeError::from)?
            }
        }
        Ok(self.runtime.drain_events())
    }

    fn queue_message(&mut self, destination: [u8; 16], data: &[u8]) -> Result<u32, NodeError> {
        self.runtime.queue_message(destination, data).map_err(NodeError::from)
    }

    fn has_outbound_capacity(&self, needed_slots: usize) -> bool {
        let capacity = self.runtime.config().max_outbound_queue;
        let used = self.runtime.pending_outbound_len();
        capacity.saturating_sub(used) >= needed_slots
    }

    fn status(&self, log_level: NodeLogLevel) -> NodeStatus {
        NodeStatus {
            run_state: NodeRunState::Running,
            epoch: self.epoch,
            lifecycle_state: Some(self.runtime.lifecycle_state()),
            pending_outbound: self.runtime.pending_outbound_len(),
            stats: self.runtime.stats(),
            log_level,
        }
    }
}

#[derive(Debug, Clone)]
enum PendingSignal {
    NodeStopped,
    NodeRestarted { epoch: u64 },
}

struct SubscriptionState {
    next_event_id: u64,
    pending_signals: VecDeque<PendingSignal>,
}

#[cfg(feature = "std")]
struct DriverState {
    epoch: u64,
    stop_requested: bool,
    start_instant: Instant,
    handle: Option<JoinHandle<()>>,
}

struct NodeState {
    epoch: u64,
    session: Option<RuntimeSession>,
    log_level: NodeLogLevel,
    next_event_id: u64,
    next_subscription_id: u64,
    last_now_ms: u64,
    event_capacity: usize,
    event_log: VecDeque<NodeEvent>,
    subscriptions: BTreeMap<u64, SubscriptionState>,
    #[cfg(feature = "std")]
    driver: Option<DriverState>,
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            epoch: 0,
            session: None,
            log_level: NodeLogLevel::Info,
            next_event_id: 1,
            next_subscription_id: 1,
            last_now_ms: 0,
            event_capacity: RuntimeConfig::default().max_events,
            event_log: VecDeque::new(),
            subscriptions: BTreeMap::new(),
            #[cfg(feature = "std")]
            driver: None,
        }
    }
}

#[cfg(feature = "std")]
struct StdNodeInner {
    state: Mutex<NodeState>,
    condvar: Condvar,
}

#[cfg(feature = "std")]
type SharedNode = Arc<StdNodeInner>;

#[cfg(not(feature = "std"))]
type SharedNode = Rc<RefCell<NodeState>>;

pub struct EmbeddedNode {
    inner: SharedNode,
}

impl Clone for EmbeddedNode {
    fn clone(&self) -> Self {
        Self { inner: clone_inner(&self.inner) }
    }
}

impl Default for EmbeddedNode {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EventSubscription {
    inner: SharedNode,
    subscription_id: u64,
}

impl Clone for EventSubscription {
    fn clone(&self) -> Self {
        Self { inner: clone_inner(&self.inner), subscription_id: self.subscription_id }
    }
}

impl EmbeddedNode {
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        let inner = Arc::new(StdNodeInner {
            state: Mutex::new(NodeState::default()),
            condvar: Condvar::new(),
        });

        #[cfg(not(feature = "std"))]
        let inner = Rc::new(RefCell::new(NodeState::default()));

        Self { inner }
    }

    pub fn start(&self, config: NodeConfig) -> Result<(), NodeError> {
        let next_epoch = self.with_state(|state| {
            if state.session.is_some() {
                return Err(NodeError::AlreadyRunning);
            }
            let epoch = state.epoch.saturating_add(1);
            let session = RuntimeSession::new(epoch, &config)?;
            state.epoch = epoch;
            state.session = Some(session);
            state.event_capacity = config.runtime.max_events;
            state.last_now_ms = 0;
            signal_generation_change(state, epoch);
            push_event_locked(
                state,
                NodeEventKind::StatusChanged {
                    run_state: NodeRunState::Running,
                    lifecycle_state: Some(NodeLifecycleState::Boot),
                },
                None,
                0,
            );
            Ok(epoch)
        })?;
        self.notify_waiters();
        #[cfg(feature = "std")]
        self.start_driver(next_epoch);
        #[cfg(not(feature = "std"))]
        let _ = next_epoch;
        Ok(())
    }

    pub fn stop(&self) -> Result<(), NodeError> {
        #[cfg(feature = "std")]
        let handle = self.with_state(|state| {
            let handle = stop_driver_locked(state);
            if state.session.is_some() {
                state.session = None;
                signal_stopped(state);
                push_event_locked(
                    state,
                    NodeEventKind::StatusChanged {
                        run_state: NodeRunState::Stopped,
                        lifecycle_state: None,
                    },
                    None,
                    state.last_now_ms,
                );
            }
            Ok(handle)
        })?;

        #[cfg(not(feature = "std"))]
        self.with_state(|state| {
            if state.session.is_some() {
                state.session = None;
                signal_stopped(state);
                push_event_locked(
                    state,
                    NodeEventKind::StatusChanged {
                        run_state: NodeRunState::Stopped,
                        lifecycle_state: None,
                    },
                    None,
                    state.last_now_ms,
                );
            }
            Ok(())
        })?;

        self.notify_waiters();

        #[cfg(feature = "std")]
        join_driver(handle);

        Ok(())
    }

    pub fn restart(&self, config: NodeConfig) -> Result<(), NodeError> {
        self.stop()?;
        self.start(config)
    }

    pub fn get_status(&self) -> NodeStatus {
        self.with_state_read(|state| {
            state.session.as_ref().map_or(
                NodeStatus {
                    run_state: NodeRunState::Stopped,
                    epoch: state.epoch,
                    lifecycle_state: None,
                    pending_outbound: 0,
                    stats: RuntimeStats::default(),
                    log_level: state.log_level,
                },
                |session| session.status(state.log_level),
            )
        })
    }

    pub fn send(
        &self,
        destination: [u8; 16],
        data: &[u8],
        _options: SendOptions,
    ) -> Result<NodeOperationReceipt, NodeError> {
        let receipt = self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            if !session.has_outbound_capacity(1) {
                return Err(NodeError::QueuePressure);
            }
            let sequence = session.queue_message(destination, data)?;
            Ok(NodeOperationReceipt {
                operation: NodeOperationKind::Send,
                operation_id: u64::from(sequence),
                epoch: session.epoch,
                accepted_bytes: data.len(),
                queued: true,
                target_count: 1,
            })
        })?;
        self.notify_waiters();
        Ok(receipt)
    }

    pub fn broadcast(
        &self,
        data: &[u8],
        options: BroadcastOptions,
    ) -> Result<NodeOperationReceipt, NodeError> {
        if options.destinations.is_empty() {
            return Err(NodeError::InvalidConfig);
        }
        let receipt = self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            if !session.has_outbound_capacity(options.destinations.len()) {
                return Err(NodeError::QueuePressure);
            }
            let mut last_sequence = 0_u64;
            for destination in &options.destinations {
                last_sequence = u64::from(session.queue_message(*destination, data)?);
            }
            Ok(NodeOperationReceipt {
                operation: NodeOperationKind::Broadcast,
                operation_id: last_sequence,
                epoch: session.epoch,
                accepted_bytes: data.len(),
                queued: true,
                target_count: u32::try_from(options.destinations.len()).unwrap_or(u32::MAX),
            })
        })?;
        self.notify_waiters();
        Ok(receipt)
    }

    pub fn set_log_level(&self, level: NodeLogLevel) -> Result<(), NodeError> {
        self.with_state(|state| {
            state.log_level = level;
            push_event_locked(
                state,
                NodeEventKind::Log { level, code: 0 },
                None,
                state.last_now_ms,
            );
            Ok(())
        })?;
        self.notify_waiters();
        Ok(())
    }

    pub fn subscribe_events(&self) -> Result<EventSubscription, NodeError> {
        let subscription_id = self.with_state(|state| {
            let id = state.next_subscription_id;
            state.next_subscription_id = state.next_subscription_id.saturating_add(1);
            state.subscriptions.insert(
                id,
                SubscriptionState {
                    next_event_id: state.next_event_id,
                    pending_signals: VecDeque::new(),
                },
            );
            Ok(id)
        })?;

        Ok(EventSubscription { inner: clone_inner(&self.inner), subscription_id })
    }

    pub fn tick(&self, now_ms: u64) -> Result<(), NodeError> {
        self.with_state(|state| {
            ensure_manual_progression_allowed(state)?;
            let events = {
                let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
                session.tick(now_ms)?
            };
            state.last_now_ms = now_ms;
            append_runtime_events_locked(state, events);
            Ok(())
        })?;
        self.notify_waiters();
        Ok(())
    }

    pub fn set_link_state(&self, state_value: LinkState) -> Result<(), NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.backend.set_link_state(state_value);
            Ok(())
        })
    }

    pub fn set_network_provisioned(&self, provisioned: bool) -> Result<(), NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.runtime.set_network_provisioned(provisioned);
            Ok(())
        })
    }

    pub fn set_ble_recovery_active(&self, active: bool) -> Result<(), NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.runtime.set_ble_recovery_active(active);
            Ok(())
        })
    }

    pub fn push_inbound_wire(&self, bytes: &[u8]) -> Result<(), NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.backend.push_inbound_wire(bytes)
        })
    }

    pub fn take_outbound_wire(&self) -> Result<Option<Vec<u8>>, NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            Ok(session.backend.take_outbound_wire())
        })
    }

    pub fn link_state(&self) -> Result<LinkState, NodeError> {
        self.with_state_read_result(|state| {
            let session = state.session.as_ref().ok_or(NodeError::NotRunning)?;
            Ok(session.backend.link_state())
        })
    }

    pub fn capability_supports_blocking_next(&self) -> bool {
        cfg!(feature = "std")
    }

    pub fn capability_supports_managed_runtime(&self) -> bool {
        cfg!(feature = "std")
    }

    pub fn capability_supports_event_gap_signaling(&self) -> bool {
        true
    }

    #[cfg(feature = "std")]
    fn start_driver(&self, epoch: u64) {
        let start_instant = Instant::now();
        if let Ok(mut state) = self.inner.state.lock() {
            state.driver =
                Some(DriverState { epoch, stop_requested: false, start_instant, handle: None });
        }

        let inner = Arc::clone(&self.inner);
        let handle = thread::spawn(move || loop {
            let continue_running = driver_tick(&inner, epoch);
            if !continue_running {
                break;
            }
            thread::sleep(DRIVER_TICK_SLEEP);
        });

        if let Ok(mut state) = self.inner.state.lock() {
            if let Some(driver) = state.driver.as_mut() {
                if driver.epoch == epoch {
                    driver.handle = Some(handle);
                    return;
                }
            }
        }

        let _ = handle.join();
    }

    #[cfg(feature = "std")]
    fn notify_waiters(&self) {
        self.inner.condvar.notify_all();
    }

    #[cfg(not(feature = "std"))]
    fn notify_waiters(&self) {}

    #[cfg(feature = "std")]
    fn with_state<R>(
        &self,
        f: impl FnOnce(&mut NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let mut state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
        f(&mut state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state<R>(
        &self,
        f: impl FnOnce(&mut NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let mut state = self.inner.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
        f(&mut state)
    }

    #[cfg(feature = "std")]
    fn with_state_read<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        let state = self.inner.state.lock().expect("node state poisoned");
        f(&state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state_read<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        let state = self.inner.borrow();
        f(&state)
    }

    #[cfg(feature = "std")]
    fn with_state_read_result<R>(
        &self,
        f: impl FnOnce(&NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
        f(&state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state_read_result<R>(
        &self,
        f: impl FnOnce(&NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let state = self.inner.try_borrow().map_err(|_| NodeError::InternalError)?;
        f(&state)
    }
}

impl EventSubscription {
    pub fn next(&self, timeout_ms: u64) -> Result<PollResult, NodeError> {
        #[cfg(feature = "std")]
        {
            if timeout_ms > MAX_BLOCKING_TIMEOUT_MS {
                return Ok(PollResult::Timeout);
            }

            let Some(deadline) = Instant::now().checked_add(Duration::from_millis(timeout_ms))
            else {
                return Ok(PollResult::Timeout);
            };
            let mut state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
            loop {
                let result = next_poll_result_locked(&mut state, self.subscription_id);
                if !matches!(result, PollResult::Timeout) || timeout_ms == 0 {
                    return Ok(result);
                }
                let now = Instant::now();
                if now >= deadline {
                    return Ok(PollResult::Timeout);
                }
                let wait = deadline.saturating_duration_since(now);
                let (next_state, _) = self
                    .inner
                    .condvar
                    .wait_timeout(state, wait)
                    .map_err(|_| NodeError::InternalError)?;
                state = next_state;
            }
        }

        #[cfg(not(feature = "std"))]
        {
            let mut state = self.inner.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
            let _ = timeout_ms;
            Ok(next_poll_result_locked(&mut state, self.subscription_id))
        }
    }

    pub fn close(&self) -> Result<(), NodeError> {
        #[cfg(feature = "std")]
        {
            let mut state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
            state.subscriptions.remove(&self.subscription_id);
            self.inner.condvar.notify_all();
            Ok(())
        }

        #[cfg(not(feature = "std"))]
        {
            let mut state = self.inner.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
            state.subscriptions.remove(&self.subscription_id);
            Ok(())
        }
    }
}

fn next_poll_result_locked(state: &mut NodeState, subscription_id: u64) -> PollResult {
    let Some(subscription) = state.subscriptions.get_mut(&subscription_id) else {
        return PollResult::Closed;
    };

    if let Some(signal) = subscription.pending_signals.pop_front() {
        return match signal {
            PendingSignal::NodeStopped => PollResult::NodeStopped,
            PendingSignal::NodeRestarted { epoch } => PollResult::NodeRestarted { epoch },
        };
    }

    let Some(first_event) = state.event_log.front() else {
        return PollResult::Timeout;
    };

    if subscription.next_event_id < first_event.event_id {
        subscription.next_event_id = first_event.event_id;
        return PollResult::Gap { next_event_id: first_event.event_id };
    }

    let Some(event) =
        state.event_log.iter().find(|event| event.event_id >= subscription.next_event_id).cloned()
    else {
        return PollResult::Timeout;
    };

    if subscription.next_event_id < event.event_id {
        subscription.next_event_id = event.event_id;
        return PollResult::Gap { next_event_id: event.event_id };
    }

    subscription.next_event_id = event.event_id.saturating_add(1);
    PollResult::Event(event)
}

fn append_runtime_events_locked(state: &mut NodeState, events: Vec<RuntimeEvent>) {
    let now_ms = state.last_now_ms;
    for event in events {
        match event {
            RuntimeEvent::LifecycleChanged { to, .. } => push_event_locked(
                state,
                NodeEventKind::StatusChanged {
                    run_state: NodeRunState::Running,
                    lifecycle_state: Some(to),
                },
                None,
                now_ms,
            ),
            RuntimeEvent::FrameSent { kind, sequence, bytes } => push_event_locked(
                state,
                NodeEventKind::PacketSent { frame_kind: kind, sequence, bytes },
                Some(u64::from(sequence)),
                now_ms,
            ),
            RuntimeEvent::FrameReceived { kind, sequence, bytes } => push_event_locked(
                state,
                NodeEventKind::PacketReceived { frame_kind: kind, sequence, bytes },
                Some(u64::from(sequence)),
                now_ms,
            ),
            RuntimeEvent::FrameDeferred { kind, sequence, error }
            | RuntimeEvent::FrameRejected { kind, sequence, error } => push_event_locked(
                state,
                NodeEventKind::Error { error: NodeError::from(error), frame_kind: kind, sequence },
                Some(u64::from(sequence)),
                now_ms,
            ),
            RuntimeEvent::Bootstrapped { replay_floor } => push_event_locked(
                state,
                NodeEventKind::Extension {
                    extension_id: NODE_EXTENSION_ID_BOOTSTRAPPED,
                    value0: replay_floor,
                    value1: 0,
                },
                None,
                now_ms,
            ),
            RuntimeEvent::AnnounceQueued { sequence }
            | RuntimeEvent::MessageQueued { sequence, .. } => {
                push_event_locked(
                    state,
                    NodeEventKind::Extension {
                        extension_id: NODE_EXTENSION_ID_MESSAGE_QUEUED,
                        value0: u64::from(sequence),
                        value1: 0,
                    },
                    Some(u64::from(sequence)),
                    now_ms,
                );
            }
            RuntimeEvent::AnnounceReceived { sequence, bytes }
            | RuntimeEvent::LxmfMessageReceived { sequence, body_bytes: bytes, .. } => {
                push_event_locked(
                    state,
                    NodeEventKind::Extension {
                        extension_id: NODE_EXTENSION_ID_RECEIVED_SUMMARY,
                        value0: u64::from(sequence),
                        value1: bytes as u64,
                    },
                    Some(u64::from(sequence)),
                    now_ms,
                );
            }
        }
    }
}

fn push_event_locked(
    state: &mut NodeState,
    kind: NodeEventKind,
    operation_id: Option<u64>,
    occurred_at_ms: u64,
) {
    if let NodeEventKind::Extension { extension_id, .. } = &kind {
        debug_assert!(is_valid_extension_id(*extension_id));
    }
    if state.event_log.len() >= state.event_capacity {
        state.event_log.pop_front();
    }
    state.event_log.push_back(NodeEvent {
        event_id: state.next_event_id,
        epoch: state.epoch,
        occurred_at_ms,
        operation_id,
        kind,
    });
    state.next_event_id = state.next_event_id.saturating_add(1);
}

#[cfg(feature = "std")]
fn ensure_manual_progression_allowed(state: &NodeState) -> Result<(), NodeError> {
    if state.driver.as_ref().is_some_and(|driver| !driver.stop_requested) {
        return Err(NodeError::ModeConflict);
    }
    Ok(())
}

#[cfg(not(feature = "std"))]
fn ensure_manual_progression_allowed(_state: &NodeState) -> Result<(), NodeError> {
    Ok(())
}

pub fn is_valid_extension_id(extension_id: u32) -> bool {
    matches!(
        extension_id,
        NODE_EXTENSION_ID_BOOTSTRAPPED
            | NODE_EXTENSION_ID_MESSAGE_QUEUED
            | NODE_EXTENSION_ID_RECEIVED_SUMMARY
    )
}

fn signal_generation_change(state: &mut NodeState, epoch: u64) {
    let next_event_id = state.next_event_id;
    for subscription in state.subscriptions.values_mut() {
        subscription.next_event_id = next_event_id;
        subscription.pending_signals.push_back(PendingSignal::NodeRestarted { epoch });
    }
}

fn signal_stopped(state: &mut NodeState) {
    let next_event_id = state.next_event_id;
    for subscription in state.subscriptions.values_mut() {
        subscription.next_event_id = next_event_id;
        subscription.pending_signals.push_back(PendingSignal::NodeStopped);
    }
}

#[cfg(feature = "std")]
fn driver_tick(inner: &Arc<StdNodeInner>, epoch: u64) -> bool {
    let keep_running = {
        let mut state = match inner.state.lock() {
            Ok(state) => state,
            Err(_) => return false,
        };

        let Some(driver) = state.driver.as_ref() else {
            return false;
        };
        if driver.stop_requested || driver.epoch != epoch {
            return false;
        }
        let now_ms = driver.start_instant.elapsed().as_millis() as u64;

        let events = match state.session.as_mut() {
            Some(session) if session.epoch == epoch => match session.tick(now_ms) {
                Ok(events) => events,
                Err(err) => {
                    state.last_now_ms = now_ms;
                    push_event_locked(
                        &mut state,
                        NodeEventKind::Error { error: err, frame_kind: 0, sequence: 0 },
                        None,
                        now_ms,
                    );
                    Vec::new()
                }
            },
            _ => return false,
        };

        state.last_now_ms = now_ms;
        append_runtime_events_locked(&mut state, events);
        true
    };

    inner.condvar.notify_all();

    keep_running
}

#[cfg(feature = "std")]
fn stop_driver_locked(state: &mut NodeState) -> Option<JoinHandle<()>> {
    let driver = state.driver.as_mut()?;
    driver.stop_requested = true;
    driver.handle.take()
}

#[cfg(feature = "std")]
fn join_driver(handle: Option<JoinHandle<()>>) {
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

#[cfg(feature = "std")]
fn clone_inner(inner: &Arc<StdNodeInner>) -> Arc<StdNodeInner> {
    Arc::clone(inner)
}

#[cfg(not(feature = "std"))]
fn clone_inner(inner: &Rc<RefCell<NodeState>>) -> Rc<RefCell<NodeState>> {
    Rc::clone(inner)
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{
        BleNodeBackendConfig, BroadcastOptions, EmbeddedNode, NodeBackendConfig, NodeConfig,
        NodeError, NodeEventKind, NodeLogLevel, NodeRunState, NodeTransportMode, PollResult,
        SendOptions,
    };
    use crate::{CaptureDefaults, RuntimeConfig};
    use rns_embedded_core::packet::decode_frame;
    use rns_embedded_core::transport::LinkState;

    #[cfg(feature = "std")]
    use std::{thread, time::Duration};

    fn config() -> NodeConfig {
        NodeConfig {
            runtime: RuntimeConfig {
                store_identity: [0x21; 32],
                lxmf_address: [0x42; 16],
                node_mode: NodeTransportMode::BleOnly,
                announce_interval_ms: 1_000,
                max_outbound_queue: 8,
                max_events: 16,
                capture_defaults: CaptureDefaults::default(),
            },
            backend: NodeBackendConfig::Ble(BleNodeBackendConfig::default()),
        }
    }

    #[test]
    fn node_starts_sends_and_exposes_ble_wire() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        assert_eq!(node.get_status().run_state, NodeRunState::Stopped);

        node.start(config()).expect("start");
        node.set_link_state(LinkState::Up).expect("link up");
        let receipt = node.send([0x99; 16], b"hello", SendOptions).expect("send");
        assert_eq!(receipt.epoch, 1);
        assert_eq!(receipt.target_count, 1);

        #[cfg(feature = "std")]
        thread::sleep(Duration::from_millis(60));
        #[cfg(not(feature = "std"))]
        node.tick(0).expect("tick");

        let first = node.take_outbound_wire().expect("take outbound").expect("frame");
        let second = node.take_outbound_wire().expect("take outbound").expect("frame");
        let decoded_first = decode_frame(&first).expect("decode first");
        let decoded_second = decode_frame(&second).expect("decode second");
        assert_eq!(decoded_first.kind, crate::FRAME_KIND_LXMF_MESSAGE);
        assert_eq!(decoded_second.kind, crate::FRAME_KIND_ANNOUNCE);

        let status = node.get_status();
        assert_eq!(status.run_state, NodeRunState::Running);
        assert_eq!(status.epoch, 1);

        assert_eq!(sub.next(0).expect("poll"), PollResult::NodeRestarted { epoch: 1 });
    }

    #[test]
    fn restart_increments_epoch_and_stop_is_idempotent() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        assert_eq!(node.get_status().epoch, 1);

        node.restart(config()).expect("restart");
        assert_eq!(node.get_status().epoch, 2);
        assert_eq!(sub.next(0).expect("signal"), PollResult::NodeRestarted { epoch: 1 });
        assert_eq!(sub.next(0).expect("signal"), PollResult::NodeStopped);
        assert_eq!(sub.next(0).expect("signal"), PollResult::NodeRestarted { epoch: 2 });

        node.stop().expect("stop");
        node.stop().expect("stop twice");
        let status = node.get_status();
        assert_eq!(status.run_state, NodeRunState::Stopped);
        assert_eq!(status.epoch, 2);
    }

    #[test]
    fn broadcast_requires_destinations_and_tracks_log_level() {
        let node = EmbeddedNode::new();
        node.start(config()).expect("start");
        node.set_log_level(NodeLogLevel::Debug).expect("log level");

        let err =
            node.broadcast(b"hello", BroadcastOptions::default()).expect_err("empty broadcast");
        assert_eq!(err, NodeError::InvalidConfig);

        let receipt = node
            .broadcast(b"hello", BroadcastOptions { destinations: vec![[0x11; 16], [0x22; 16]] })
            .expect("broadcast");
        assert_eq!(receipt.target_count, 2);
        assert_eq!(node.get_status().log_level, NodeLogLevel::Debug);
        assert_eq!(node.get_status().pending_outbound, 2);
    }

    #[test]
    fn queue_pressure_is_stable_and_broadcast_is_all_or_nothing() {
        let mut small = config();
        small.runtime.max_outbound_queue = 1;

        let node = EmbeddedNode::new();
        node.start(small).expect("start");

        let receipt = node.send([0xAA; 16], b"one", SendOptions).expect("first send");
        assert_eq!(receipt.target_count, 1);
        assert_eq!(node.get_status().pending_outbound, 1);

        let err = node.send([0xBB; 16], b"two", SendOptions).expect_err("queue pressure");
        assert_eq!(err, NodeError::QueuePressure);
        assert_eq!(node.get_status().pending_outbound, 1);

        node.stop().expect("stop");

        let node = EmbeddedNode::new();
        node.start(config()).expect("start");
        let err = node
            .broadcast(b"hello", BroadcastOptions { destinations: vec![[0x11; 16]; 9] })
            .expect_err("broadcast queue pressure");
        assert_eq!(err, NodeError::QueuePressure);
        assert_eq!(node.get_status().pending_outbound, 0);
    }

    #[test]
    fn subscriptions_observe_runtime_events_and_close() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        node.set_link_state(LinkState::Up).expect("link up");

        #[cfg(feature = "std")]
        thread::sleep(Duration::from_millis(60));
        #[cfg(not(feature = "std"))]
        node.tick(0).expect("tick");

        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { epoch: 1 }));
        assert!(matches!(
            sub.next(0).expect("event"),
            PollResult::Event(crate::node::NodeEvent {
                kind: NodeEventKind::StatusChanged { .. },
                ..
            })
        ));

        sub.close().expect("close");
        assert_eq!(sub.next(0).expect("closed"), PollResult::Closed);
    }

    #[cfg(feature = "std")]
    #[test]
    fn subscription_timeout_overflow_returns_timeout() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");

        assert_eq!(sub.next(u64::MAX).expect("overflow timeout"), PollResult::Timeout);
    }

    #[cfg(feature = "std")]
    #[test]
    fn managed_mode_rejects_manual_tick() {
        let node = EmbeddedNode::new();
        node.start(config()).expect("start");

        assert_eq!(node.tick(0).expect_err("mode conflict"), NodeError::ModeConflict);
    }

    #[cfg(feature = "std")]
    #[test]
    fn blocked_next_wakes_on_stop() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        thread::sleep(Duration::from_millis(60));
        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { .. }));
        while !matches!(sub.next(0).expect("drain"), PollResult::Timeout) {}

        let waiter = sub.clone();
        let handle = thread::spawn(move || waiter.next(5_000).expect("waiter"));
        thread::sleep(Duration::from_millis(50));
        node.stop().expect("stop");

        assert_eq!(handle.join().expect("join"), PollResult::NodeStopped);
    }

    #[cfg(feature = "std")]
    #[test]
    fn blocked_next_wakes_on_restart() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        thread::sleep(Duration::from_millis(60));
        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { .. }));
        while !matches!(sub.next(0).expect("drain"), PollResult::Timeout) {}

        let waiter = sub.clone();
        let handle = thread::spawn(move || waiter.next(5_000).expect("waiter"));
        thread::sleep(Duration::from_millis(50));
        node.restart(config()).expect("restart");

        assert_eq!(handle.join().expect("join"), PollResult::NodeStopped);
        assert!(matches!(
            sub.next(0).expect("restart signal"),
            PollResult::NodeRestarted { epoch: 2 }
        ));
    }

    #[cfg(feature = "std")]
    #[test]
    fn closing_subscription_wakes_blocked_waiter() {
        let node = EmbeddedNode::new();
        let sub = node.subscribe_events().expect("subscribe");
        node.start(config()).expect("start");
        thread::sleep(Duration::from_millis(60));
        assert!(matches!(sub.next(0).expect("restart"), PollResult::NodeRestarted { .. }));
        while !matches!(sub.next(0).expect("drain"), PollResult::Timeout) {}

        let waiter = sub.clone();
        let handle = thread::spawn(move || waiter.next(5_000).expect("waiter"));
        thread::sleep(Duration::from_millis(50));
        sub.close().expect("close");

        assert_eq!(handle.join().expect("join"), PollResult::Closed);
    }
}
