use crate::{
    RuntimeConfig, RuntimeStats,
    ble::{BleShimConfig, BleShimTransport},
    constants::DEFAULT_CAPTURE_MAX_BYTES,
};
use alloc::{string::String, vec::Vec};
use rns_embedded_core::{
    EmbeddedError,
    store::JournaledEmbeddedStore,
    transport::LinkState,
};

#[cfg(not(feature = "std"))]
use core::cell::RefCell;

#[cfg(feature = "std")]
use std::sync::Mutex;

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
        Self {
            max_bytes: DEFAULT_CAPTURE_MAX_BYTES,
        }
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
}

impl From<EmbeddedError> for NodeError {
    fn from(value: EmbeddedError) -> Self {
        match value {
            EmbeddedError::InvalidInput | EmbeddedError::InvalidArgument | EmbeddedError::Unsupported => {
                Self::InvalidConfig
            }
            EmbeddedError::Timeout => Self::Timeout,
            EmbeddedError::Backpressure | EmbeddedError::Disconnected => Self::NetworkError,
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

enum NodeBackend {
    Ble(BleShimTransport),
}

impl NodeBackend {
    fn set_link_state(&mut self, state: LinkState) {
        match self {
            Self::Ble(transport) => transport.set_link_state(state),
        }
    }

    fn push_inbound_wire(&mut self, bytes: &[u8]) -> Result<(), NodeError> {
        match self {
            Self::Ble(transport) => transport.push_inbound_wire(bytes).map_err(NodeError::from),
        }
    }

    fn take_outbound_wire(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Ble(transport) => transport.take_outbound_wire(),
        }
    }

    fn link_state(&self) -> LinkState {
        match self {
            Self::Ble(transport) => rns_embedded_core::transport::EmbeddedTransport::link_state(transport),
        }
    }
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
                NodeBackend::Ble(BleShimTransport::new(BleShimConfig::from(ble)).map_err(NodeError::from)?)
            }
            #[cfg(feature = "std")]
            NodeBackendConfig::TcpClient(_) | NodeBackendConfig::TcpServer(_) => {
                return Err(NodeError::InvalidConfig);
            }
        };

        Ok(Self {
            epoch,
            runtime,
            store: JournaledEmbeddedStore::new(),
            backend,
        })
    }

    fn tick(&mut self, now_ms: u64) -> Result<(), NodeError> {
        match &mut self.backend {
            NodeBackend::Ble(transport) => self
                .runtime
                .tick(now_ms, transport, &mut self.store)
                .map_err(NodeError::from),
        }
    }

    fn queue_message(&mut self, destination: [u8; 16], data: &[u8]) -> Result<u32, NodeError> {
        self.runtime
            .queue_message(destination, data)
            .map_err(NodeError::from)
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

struct NodeState {
    epoch: u64,
    session: Option<RuntimeSession>,
    log_level: NodeLogLevel,
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            epoch: 0,
            session: None,
            log_level: NodeLogLevel::Info,
        }
    }
}

#[cfg(feature = "std")]
type NodeStateCell = Mutex<NodeState>;

#[cfg(not(feature = "std"))]
type NodeStateCell = RefCell<NodeState>;

pub struct EmbeddedNode {
    state: NodeStateCell,
}

impl Default for EmbeddedNode {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddedNode {
    pub fn new() -> Self {
        Self {
            state: NodeStateCell::new(NodeState::default()),
        }
    }

    pub fn start(&self, config: NodeConfig) -> Result<(), NodeError> {
        self.with_state(|state| {
            if state.session.is_some() {
                return Err(NodeError::AlreadyRunning);
            }
            let epoch = state.epoch.saturating_add(1);
            let session = RuntimeSession::new(epoch, &config)?;
            state.epoch = epoch;
            state.session = Some(session);
            Ok(())
        })
    }

    pub fn stop(&self) -> Result<(), NodeError> {
        self.with_state(|state| {
            state.session = None;
            Ok(())
        })
    }

    pub fn restart(&self, config: NodeConfig) -> Result<(), NodeError> {
        self.with_state(|state| {
            let epoch = state.epoch.saturating_add(1);
            let session = RuntimeSession::new(epoch, &config)?;
            state.epoch = epoch;
            state.session = Some(session);
            Ok(())
        })
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
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            let sequence = session.queue_message(destination, data)?;
            Ok(NodeOperationReceipt {
                operation: NodeOperationKind::Send,
                operation_id: u64::from(sequence),
                epoch: session.epoch,
                accepted_bytes: data.len(),
                queued: true,
                target_count: 1,
            })
        })
    }

    pub fn broadcast(
        &self,
        data: &[u8],
        options: BroadcastOptions,
    ) -> Result<NodeOperationReceipt, NodeError> {
        if options.destinations.is_empty() {
            return Err(NodeError::InvalidConfig);
        }
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
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
        })
    }

    pub fn set_log_level(&self, level: NodeLogLevel) -> Result<(), NodeError> {
        self.with_state(|state| {
            state.log_level = level;
            Ok(())
        })
    }

    pub fn tick(&self, now_ms: u64) -> Result<(), NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.tick(now_ms)
        })
    }

    pub fn set_link_state(&self, state: LinkState) -> Result<(), NodeError> {
        self.with_state(|node| {
            let session = node.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.backend.set_link_state(state);
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

    #[cfg(feature = "std")]
    fn with_state<R>(
        &self,
        f: impl FnOnce(&mut NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let mut state = self.state.lock().map_err(|_| NodeError::InternalError)?;
        f(&mut state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state<R>(
        &self,
        f: impl FnOnce(&mut NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let mut state = self.state.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
        f(&mut state)
    }

    #[cfg(feature = "std")]
    fn with_state_read<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        let state = self.state.lock().expect("node state poisoned");
        f(&state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state_read<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        let state = self.state.borrow();
        f(&state)
    }

    #[cfg(feature = "std")]
    fn with_state_read_result<R>(
        &self,
        f: impl FnOnce(&NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let state = self.state.lock().map_err(|_| NodeError::InternalError)?;
        f(&state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state_read_result<R>(
        &self,
        f: impl FnOnce(&NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let state = self.state.try_borrow().map_err(|_| NodeError::InternalError)?;
        f(&state)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BleNodeBackendConfig, BroadcastOptions, EmbeddedNode, NodeBackendConfig, NodeConfig,
        NodeError, NodeLogLevel, NodeRunState, NodeTransportMode, SendOptions,
    };
    use crate::{CaptureDefaults, RuntimeConfig};
    use rns_embedded_core::packet::decode_frame;
    use rns_embedded_core::transport::LinkState;

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
        assert_eq!(node.get_status().run_state, NodeRunState::Stopped);

        node.start(config()).expect("start");
        node.set_link_state(LinkState::Up).expect("link up");
        let receipt = node
            .send([0x99; 16], b"hello", SendOptions)
            .expect("send");
        assert_eq!(receipt.epoch, 1);
        assert_eq!(receipt.target_count, 1);

        node.tick(0).expect("tick");

        let first = node
            .take_outbound_wire()
            .expect("take outbound")
            .expect("frame");
        let second = node
            .take_outbound_wire()
            .expect("take outbound")
            .expect("frame");
        let decoded_first = decode_frame(&first).expect("decode first");
        let decoded_second = decode_frame(&second).expect("decode second");
        assert_eq!(decoded_first.kind, crate::FRAME_KIND_LXMF_MESSAGE);
        assert_eq!(decoded_second.kind, crate::FRAME_KIND_ANNOUNCE);

        let status = node.get_status();
        assert_eq!(status.run_state, NodeRunState::Running);
        assert_eq!(status.epoch, 1);
        assert_eq!(status.pending_outbound, 0);
    }

    #[test]
    fn restart_increments_epoch_and_stop_is_idempotent() {
        let node = EmbeddedNode::new();
        node.start(config()).expect("start");
        assert_eq!(node.get_status().epoch, 1);

        node.restart(config()).expect("restart");
        assert_eq!(node.get_status().epoch, 2);

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

        let err = node
            .broadcast(b"hello", BroadcastOptions::default())
            .expect_err("empty broadcast");
        assert_eq!(err, NodeError::InvalidConfig);

        let receipt = node
            .broadcast(
                b"hello",
                BroadcastOptions {
                    destinations: vec![[0x11; 16], [0x22; 16]],
                },
            )
            .expect("broadcast");
        assert_eq!(receipt.target_count, 2);
        assert_eq!(node.get_status().log_level, NodeLogLevel::Debug);
        assert_eq!(node.get_status().pending_outbound, 2);
    }
}
