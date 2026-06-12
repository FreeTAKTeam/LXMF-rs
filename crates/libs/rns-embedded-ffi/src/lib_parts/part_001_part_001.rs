use alloc::{boxed::Box, string::String};

use rns_embedded_core::{store::JournaledEmbeddedStore, transport::LinkState, EmbeddedError};

use rns_embedded_runtime::{
    ble::{BleShimConfig, BleShimTransport},
    node::{CaptureDefaults, NodeLifecycleState, NodeTransportMode},
    BleNodeBackendConfig, BroadcastOptions, EmbeddedNode, EmbeddedNodeRuntime, EventSubscription,
    NodeBackendConfig, NodeConfig, NodeError, NodeEvent, NodeEventKind, NodeLogLevel, NodeRunState,
    NodeStatus, PollResult, RuntimeConfig, SendOptions, TcpClientConfig, TcpServerConfig,
};

#[cfg(not(feature = "std"))]
use core::panic::PanicInfo;

#[cfg(feature = "std")]
use std::panic::{catch_unwind, AssertUnwindSafe};

#[cfg(not(feature = "std"))]
use critical_section::RawRestoreState;

#[cfg(not(feature = "std"))]
use embedded_alloc::LlffHeap;

#[cfg(not(feature = "std"))]
#[global_allocator]
static RNS_ALLOCATOR: LlffHeap = LlffHeap::empty();

#[cfg(not(feature = "std"))]
static mut RNS_ALLOCATOR_HEAP: [u8; 48 * 1024] = [0; 48 * 1024];

#[cfg(not(feature = "std"))]
static mut RNS_ALLOCATOR_READY: bool = false;

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(feature = "std"))]
struct NoopCriticalSection;

#[cfg(not(feature = "std"))]
critical_section::set_impl!(NoopCriticalSection);

#[cfg(not(feature = "std"))]
// SAFETY: this shim runs on single-threaded embedded builds where callers do not
// rely on nested interrupt masking; the no-op critical section only gates local
// allocator/runtime bootstrap paths in this crate.
unsafe impl critical_section::Impl for NoopCriticalSection {
    // SAFETY: acquire does not modify machine state and the paired restore token
    // is the unit type, so callers can only observe the no-op contract above.
    unsafe fn acquire() -> RawRestoreState {}

    // SAFETY: release is paired with the no-op acquire implementation and has no
    // machine state to restore for this single-threaded shim.
    unsafe fn release(_restore_state: RawRestoreState) {}
}

#[repr(C)]
pub struct RnsEmbeddedNodeConfig {
    pub store_identity: [u8; 32],
    pub lxmf_address: [u8; 16],
    pub node_mode: RnsEmbeddedNodeMode,
    pub announce_interval_ms: u64,
    pub max_outbound_queue: usize,
    pub max_events: usize,
    pub capture_default_max_bytes: u32,
    pub ble_mtu_hint: u16,
    pub ble_max_inbound_frames: usize,
    pub ble_max_outbound_frames: usize,
    pub ble_ordered_delivery: bool,
    pub tcp_host: [u8; 256],
    pub tcp_port: u16,
    pub tcp_listen_port: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedNodeMode {
    BleOnly = 0,
    TcpClient = 1,
    TcpServer = 2,
}

impl Default for RnsEmbeddedNodeConfig {
    fn default() -> Self {
        let runtime = RuntimeConfig::default();
        let ble = BleShimConfig::default();
        Self {
            store_identity: runtime.store_identity,
            lxmf_address: runtime.lxmf_address,
            node_mode: RnsEmbeddedNodeMode::BleOnly,
            announce_interval_ms: runtime.announce_interval_ms,
            max_outbound_queue: runtime.max_outbound_queue,
            max_events: runtime.max_events,
            capture_default_max_bytes: runtime.capture_defaults.max_bytes,
            ble_mtu_hint: ble.mtu_hint,
            ble_max_inbound_frames: ble.max_inbound_frames,
            ble_max_outbound_frames: ble.max_outbound_frames,
            ble_ordered_delivery: ble.ordered_delivery,
            tcp_host: [0; 256],
            tcp_port: 0,
            tcp_listen_port: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedLinkState {
    Down = 0,
    Connecting = 1,
    Up = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedLifecycleState {
    Boot = 0,
    Unprovisioned = 1,
    ProvisionedOffline = 2,
    TcpOnline = 3,
    BleRecovery = 4,
    FailureReconnect = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedStatus {
    Ok = 0,
    InvalidInput = 1,
    InvalidArgument = 2,
    InvalidState = 3,
    NotFound = 4,
    SeqGap = 5,
    IntegrityFailure = 6,
    ChecksumMismatch = 7,
    IdempotencyConflict = 8,
    ReplayRejected = 9,
    Timeout = 10,
    Backpressure = 11,
    Disconnected = 12,
    StorageCorruption = 13,
    Unsupported = 14,
}

const RNS_EMBEDDED_V1_ABI_VERSION: u32 = 1;

const RNS_EMBEDDED_V1_STRUCT_VERSION: u32 = 1;

const RNS_EMBEDDED_V1_CAPABILITY_SCHEMA_VERSION: u32 = 1;

const RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME: u64 = 1 << 0;

const RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT: u64 = 1 << 1;

const RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST: u64 = 1 << 2;

const RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI: u64 = 1 << 3;

const RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING: u64 = 1 << 4;

const RNS_EMBEDDED_V1_KNOWN_CAPABILITY_BITS: u64 = RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME
    | RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT
    | RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST
    | RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI
    | RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING;

const RNS_EMBEDDED_V1_DRIVER_TICK_TARGET_MS: u32 = 25;

const RNS_EMBEDDED_V1_DRIVER_TICK_MAX_MS: u32 = 50;

const RNS_EMBEDDED_V1_MAX_BLOCKING_TIMEOUT_MS: u64 = u32::MAX as u64;

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedV1RunState {
    Stopped = 0,
    Running = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedV1LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedV1EventKind {
    StatusChanged = 0,
    Log = 1,
    Error = 2,
    PacketReceived = 3,
    PacketSent = 4,
    Extension = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedV1PollResultKind {
    Event = 0,
    Timeout = 1,
    Closed = 2,
    Gap = 3,
    NodeStopped = 4,
    NodeRestarted = 5,
}
