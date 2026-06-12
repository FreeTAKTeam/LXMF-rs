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
