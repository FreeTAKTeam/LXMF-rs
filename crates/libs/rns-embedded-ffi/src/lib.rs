#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;

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

include!("generated/node_error_codes.rs");

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsEmbeddedV1NodeError {
    pub struct_size: usize,
    pub struct_version: u32,
    pub code: RnsEmbeddedV1NodeErrorCode,
    pub reserved: [u8; 16],
}

impl Default for RnsEmbeddedV1NodeError {
    fn default() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            code: RnsEmbeddedV1NodeErrorCode::Unknown,
            reserved: [0; 16],
        }
    }
}

#[repr(C)]
pub struct RnsEmbeddedV1NodeConfig {
    pub struct_size: usize,
    pub struct_version: u32,
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
    pub reserved: [u8; 28],
}

impl Default for RnsEmbeddedV1NodeConfig {
    fn default() -> Self {
        let legacy = RnsEmbeddedNodeConfig::default();
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            store_identity: legacy.store_identity,
            lxmf_address: legacy.lxmf_address,
            node_mode: legacy.node_mode,
            announce_interval_ms: legacy.announce_interval_ms,
            max_outbound_queue: legacy.max_outbound_queue,
            max_events: legacy.max_events,
            capture_default_max_bytes: legacy.capture_default_max_bytes,
            ble_mtu_hint: legacy.ble_mtu_hint,
            ble_max_inbound_frames: legacy.ble_max_inbound_frames,
            ble_max_outbound_frames: legacy.ble_max_outbound_frames,
            ble_ordered_delivery: legacy.ble_ordered_delivery,
            tcp_host: legacy.tcp_host,
            tcp_port: legacy.tcp_port,
            tcp_listen_port: legacy.tcp_listen_port,
            reserved: [0; 28],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsEmbeddedV1NodeStatus {
    pub struct_size: usize,
    pub struct_version: u32,
    pub run_state: RnsEmbeddedV1RunState,
    pub epoch: u64,
    pub lifecycle_state: RnsEmbeddedLifecycleState,
    pub pending_outbound: usize,
    pub announces_queued: u32,
    pub outbound_sent: u32,
    pub outbound_deferred: u32,
    pub inbound_accepted: u32,
    pub inbound_rejected: u32,
    pub announces_received: u32,
    pub lxmf_messages_received: u32,
    pub log_level: RnsEmbeddedV1LogLevel,
    pub reserved: [u8; 24],
}

impl Default for RnsEmbeddedV1NodeStatus {
    fn default() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            run_state: RnsEmbeddedV1RunState::Stopped,
            epoch: 0,
            lifecycle_state: RnsEmbeddedLifecycleState::Boot,
            pending_outbound: 0,
            announces_queued: 0,
            outbound_sent: 0,
            outbound_deferred: 0,
            inbound_accepted: 0,
            inbound_rejected: 0,
            announces_received: 0,
            lxmf_messages_received: 0,
            log_level: RnsEmbeddedV1LogLevel::Info,
            reserved: [0; 24],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsEmbeddedV1SendReceipt {
    pub struct_size: usize,
    pub struct_version: u32,
    pub operation_id: u64,
    pub epoch: u64,
    pub accepted_bytes: usize,
    pub queued: bool,
    pub target_count: u32,
    pub reserved: [u8; 24],
}

impl Default for RnsEmbeddedV1SendReceipt {
    fn default() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            operation_id: 0,
            epoch: 0,
            accepted_bytes: 0,
            queued: false,
            target_count: 0,
            reserved: [0; 24],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsEmbeddedV1Capabilities {
    pub struct_size: usize,
    pub struct_version: u32,
    pub abi_version: u32,
    pub capability_schema_version: u32,
    pub known_capability_bits: u64,
    pub compile_time_capability_bits: u64,
    pub capability_bits: u64,
    pub max_event_payload_bytes: u32,
    pub max_subscriptions: u32,
    pub max_blocking_timeout_ms: u64,
    pub driver_tick_target_ms: u32,
    pub driver_tick_max_ms: u32,
    pub reserved: [u8; 24],
}

impl Default for RnsEmbeddedV1Capabilities {
    fn default() -> Self {
        let mut compile_time_capability_bits =
            RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST | RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI;
        if cfg!(feature = "std") {
            compile_time_capability_bits |=
                RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME | RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT;
        }
        if cfg!(feature = "std") || cfg!(feature = "alloc") {
            compile_time_capability_bits |= RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING;
        }
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            abi_version: RNS_EMBEDDED_V1_ABI_VERSION,
            capability_schema_version: RNS_EMBEDDED_V1_CAPABILITY_SCHEMA_VERSION,
            known_capability_bits: RNS_EMBEDDED_V1_KNOWN_CAPABILITY_BITS,
            compile_time_capability_bits,
            capability_bits: compile_time_capability_bits,
            max_event_payload_bytes: 0,
            max_subscriptions: 1024,
            max_blocking_timeout_ms: if cfg!(feature = "std") {
                RNS_EMBEDDED_V1_MAX_BLOCKING_TIMEOUT_MS
            } else {
                0
            },
            driver_tick_target_ms: if cfg!(feature = "std") {
                RNS_EMBEDDED_V1_DRIVER_TICK_TARGET_MS
            } else {
                0
            },
            driver_tick_max_ms: if cfg!(feature = "std") {
                RNS_EMBEDDED_V1_DRIVER_TICK_MAX_MS
            } else {
                0
            },
            reserved: [0; 24],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsEmbeddedV1NodeEvent {
    pub struct_size: usize,
    pub struct_version: u32,
    pub kind: RnsEmbeddedV1EventKind,
    pub event_id: u64,
    pub epoch: u64,
    pub occurred_at_ms: u64,
    pub operation_id: u64,
    pub has_operation_id: bool,
    pub run_state: RnsEmbeddedV1RunState,
    pub lifecycle_state: RnsEmbeddedLifecycleState,
    pub log_level: RnsEmbeddedV1LogLevel,
    pub error_code: RnsEmbeddedV1NodeErrorCode,
    pub frame_kind: u8,
    pub sequence: u32,
    pub bytes: usize,
    pub extension_id: u32,
    pub value0: u64,
    pub value1: u64,
    pub reserved: [u8; 24],
}

impl Default for RnsEmbeddedV1NodeEvent {
    fn default() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            kind: RnsEmbeddedV1EventKind::StatusChanged,
            event_id: 0,
            epoch: 0,
            occurred_at_ms: 0,
            operation_id: 0,
            has_operation_id: false,
            run_state: RnsEmbeddedV1RunState::Stopped,
            lifecycle_state: RnsEmbeddedLifecycleState::Boot,
            log_level: RnsEmbeddedV1LogLevel::Info,
            error_code: RnsEmbeddedV1NodeErrorCode::Unknown,
            frame_kind: 0,
            sequence: 0,
            bytes: 0,
            extension_id: 0,
            value0: 0,
            value1: 0,
            reserved: [0; 24],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsEmbeddedV1PollResult {
    pub struct_size: usize,
    pub struct_version: u32,
    pub kind: RnsEmbeddedV1PollResultKind,
    pub next_event_id: u64,
    pub epoch: u64,
    pub reserved: [u8; 24],
}

impl Default for RnsEmbeddedV1PollResult {
    fn default() -> Self {
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            kind: RnsEmbeddedV1PollResultKind::Timeout,
            next_event_id: 0,
            epoch: 0,
            reserved: [0; 24],
        }
    }
}

pub struct RnsEmbeddedNode {
    runtime: EmbeddedNodeRuntime,
    store: JournaledEmbeddedStore,
    transport: BleShimTransport,
}

pub struct RnsEmbeddedV1Node {
    node: EmbeddedNode,
}

pub struct RnsEmbeddedEventSubscription {
    subscription: EventSubscription,
}

#[cfg(not(feature = "std"))]
fn ensure_allocator_ready() {
    // SAFETY: single-core bring-up for this proof path; repeated calls are idempotent
    // because the heap is initialized exactly once before any allocation-heavy entrypoint.
    unsafe {
        if !RNS_ALLOCATOR_READY {
            RNS_ALLOCATOR.init(RNS_ALLOCATOR_HEAP.as_mut_ptr() as usize, RNS_ALLOCATOR_HEAP.len());
            RNS_ALLOCATOR_READY = true;
        }
    }
}

#[cfg(feature = "std")]
fn ensure_allocator_ready() {}

#[no_mangle]
pub extern "C" fn rns_embedded_node_config_default() -> RnsEmbeddedNodeConfig {
    RnsEmbeddedNodeConfig::default()
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_config_default() -> RnsEmbeddedV1NodeConfig {
    RnsEmbeddedV1NodeConfig::default()
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_abi_version() -> u32 {
    RNS_EMBEDDED_V1_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_get_capabilities(
    out_capabilities: *mut RnsEmbeddedV1Capabilities,
) -> RnsEmbeddedStatus {
    ffi_status_boundary(|| {
        if out_capabilities.is_null() {
            return RnsEmbeddedStatus::InvalidArgument;
        }
        // SAFETY: `out_capabilities` is validated non-null above and points to
        // writable caller-provided storage for one `RnsEmbeddedV1Capabilities`.
        unsafe {
            *out_capabilities = RnsEmbeddedV1Capabilities::default();
        }
        RnsEmbeddedStatus::Ok
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_new() -> *mut RnsEmbeddedV1Node {
    ffi_ptr_boundary(|| {
        ensure_allocator_ready();
        Box::into_raw(Box::new(RnsEmbeddedV1Node { node: EmbeddedNode::new() }))
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_free(node: *mut RnsEmbeddedV1Node) {
    if node.is_null() {
        return;
    }
    // SAFETY: `node` is allocated by `rns_embedded_v1_node_new` with
    // `Box::into_raw` and this function takes back ownership exactly once.
    unsafe {
        drop(Box::from_raw(node));
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_new(
    config: *const RnsEmbeddedNodeConfig,
) -> *mut RnsEmbeddedNode {
    ensure_allocator_ready();
    if config.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: `config` is checked non-null above and points to a caller-owned
    // `RnsEmbeddedNodeConfig` that is only read during this call.
    let config = unsafe { &*config };
    let runtime = match EmbeddedNodeRuntime::new(RuntimeConfig {
        store_identity: config.store_identity,
        lxmf_address: config.lxmf_address,
        node_mode: match config.node_mode {
            RnsEmbeddedNodeMode::BleOnly => NodeTransportMode::BleOnly,
            RnsEmbeddedNodeMode::TcpClient => NodeTransportMode::TcpClient,
            RnsEmbeddedNodeMode::TcpServer => NodeTransportMode::TcpServer,
        },
        announce_interval_ms: config.announce_interval_ms,
        max_outbound_queue: config.max_outbound_queue,
        max_events: config.max_events,
        capture_defaults: CaptureDefaults { max_bytes: config.capture_default_max_bytes },
    }) {
        Ok(runtime) => runtime,
        Err(_) => return core::ptr::null_mut(),
    };
    let transport = match BleShimTransport::new(BleShimConfig {
        mtu_hint: config.ble_mtu_hint,
        max_inbound_frames: config.ble_max_inbound_frames,
        max_outbound_frames: config.ble_max_outbound_frames,
        ordered_delivery: config.ble_ordered_delivery,
    }) {
        Ok(transport) => transport,
        Err(_) => return core::ptr::null_mut(),
    };

    let node = RnsEmbeddedNode { runtime, store: JournaledEmbeddedStore::new(), transport };
    Box::into_raw(Box::new(node))
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_free(node: *mut RnsEmbeddedNode) {
    if node.is_null() {
        return;
    }
    // SAFETY: `node` originates from `Box::into_raw` in `rns_embedded_node_new`
    // and this function takes ownership exactly once to drop it.
    unsafe {
        drop(Box::from_raw(node));
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_set_link_state(
    node: *mut RnsEmbeddedNode,
    state: RnsEmbeddedLinkState,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    node.transport.set_link_state(match state {
        RnsEmbeddedLinkState::Down => LinkState::Down,
        RnsEmbeddedLinkState::Connecting => LinkState::Connecting,
        RnsEmbeddedLinkState::Up => LinkState::Up,
    });
    RnsEmbeddedStatus::Ok
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_tick(
    node: *mut RnsEmbeddedNode,
    now_ms: u64,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    map_status(node.runtime.tick(now_ms, &mut node.transport, &mut node.store))
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_set_network_provisioned(
    node: *mut RnsEmbeddedNode,
    provisioned: bool,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    node.runtime.set_network_provisioned(provisioned);
    RnsEmbeddedStatus::Ok
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_set_ble_recovery_active(
    node: *mut RnsEmbeddedNode,
    active: bool,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    node.runtime.set_ble_recovery_active(active);
    RnsEmbeddedStatus::Ok
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_get_lifecycle_state(
    node: *mut RnsEmbeddedNode,
) -> RnsEmbeddedLifecycleState {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedLifecycleState::Boot;
    };
    match node.runtime.lifecycle_state() {
        NodeLifecycleState::Boot => RnsEmbeddedLifecycleState::Boot,
        NodeLifecycleState::Unprovisioned => RnsEmbeddedLifecycleState::Unprovisioned,
        NodeLifecycleState::ProvisionedOffline => RnsEmbeddedLifecycleState::ProvisionedOffline,
        NodeLifecycleState::TcpOnline => RnsEmbeddedLifecycleState::TcpOnline,
        NodeLifecycleState::BleRecovery => RnsEmbeddedLifecycleState::BleRecovery,
        NodeLifecycleState::FailureReconnect => RnsEmbeddedLifecycleState::FailureReconnect,
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_push_inbound_wire(
    node: *mut RnsEmbeddedNode,
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    let Some(bytes) = byte_slice(bytes_ptr, bytes_len) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    map_status(node.transport.push_inbound_wire(bytes))
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_take_outbound_wire(
    node: *mut RnsEmbeddedNode,
    out_ptr: *mut u8,
    out_capacity: usize,
    out_len: *mut usize,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    if out_ptr.is_null() || out_len.is_null() {
        return RnsEmbeddedStatus::InvalidArgument;
    }

    let Some(frame) = node.transport.take_outbound_wire() else {
        // SAFETY: `out_len` is validated non-null above and points to writable caller memory.
        unsafe {
            *out_len = 0;
        }
        return RnsEmbeddedStatus::NotFound;
    };
    if frame.len() > out_capacity {
        // SAFETY: `out_len` is validated non-null above and points to writable caller memory.
        unsafe {
            *out_len = frame.len();
        }
        return RnsEmbeddedStatus::Backpressure;
    }
    // SAFETY: `out_ptr` and `out_len` are validated non-null above; caller
    // provides writable storage of at least `out_capacity` bytes. `frame.len()`
    // is checked against `out_capacity` before copying.
    unsafe {
        core::ptr::copy_nonoverlapping(frame.as_ptr(), out_ptr, frame.len());
        *out_len = frame.len();
    }
    RnsEmbeddedStatus::Ok
}

#[no_mangle]
pub extern "C" fn rns_embedded_node_queue_message(
    node: *mut RnsEmbeddedNode,
    destination_ptr: *const u8,
    body_ptr: *const u8,
    body_len: usize,
    out_sequence: *mut u32,
) -> RnsEmbeddedStatus {
    let Some(node) = node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    if destination_ptr.is_null() || out_sequence.is_null() {
        return RnsEmbeddedStatus::InvalidArgument;
    }
    // SAFETY: `destination_ptr` is validated non-null above and points to at
    // least 16 bytes of caller-owned memory for the duration of this call.
    let destination_slice = unsafe { core::slice::from_raw_parts(destination_ptr, 16) };
    let mut destination = [0_u8; 16];
    destination.copy_from_slice(destination_slice);
    let Some(body) = byte_slice(body_ptr, body_len) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    match node.runtime.queue_message(destination, body) {
        Ok(sequence) => {
            // SAFETY: `out_sequence` is validated non-null above and points to writable caller memory.
            unsafe {
                *out_sequence = sequence;
            }
            RnsEmbeddedStatus::Ok
        }
        Err(err) => map_embedded_error(err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_start(
    node: *mut RnsEmbeddedV1Node,
    config: *const RnsEmbeddedV1NodeConfig,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        if config.is_null() {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        }
        // SAFETY: `config` is validated non-null above and is only borrowed
        // immutably for the duration of this conversion/start call.
        let config = unsafe { &*config };
        let node_config = match v1_node_config(config) {
            Ok(config) => config,
            Err(err) => return set_v1_node_error(out_node_error, err),
        };
        match node.node.start(node_config) {
            Ok(()) => clear_v1_node_error(out_node_error),
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_stop(
    node: *mut RnsEmbeddedV1Node,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        match node.node.stop() {
            Ok(()) => clear_v1_node_error(out_node_error),
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_restart(
    node: *mut RnsEmbeddedV1Node,
    config: *const RnsEmbeddedV1NodeConfig,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        if config.is_null() {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        }
        // SAFETY: `config` is validated non-null above and is only borrowed
        // immutably while building the restart configuration.
        let config = unsafe { &*config };
        let node_config = match v1_node_config(config) {
            Ok(config) => config,
            Err(err) => return set_v1_node_error(out_node_error, err),
        };
        match node.node.restart(node_config) {
            Ok(()) => clear_v1_node_error(out_node_error),
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_get_status(
    node: *mut RnsEmbeddedV1Node,
    out_status: *mut RnsEmbeddedV1NodeStatus,
) -> RnsEmbeddedStatus {
    ffi_status_boundary(|| {
        let Some(node) = v1_node_mut(node) else {
            return RnsEmbeddedStatus::InvalidArgument;
        };
        if out_status.is_null() {
            return RnsEmbeddedStatus::InvalidArgument;
        }
        let status = node.node.get_status();
        // SAFETY: `out_status` is validated non-null above and points to writable
        // caller storage for one `RnsEmbeddedV1NodeStatus`.
        unsafe {
            *out_status = map_v1_status(status);
        }
        RnsEmbeddedStatus::Ok
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_send(
    node: *mut RnsEmbeddedV1Node,
    destination_ptr: *const u8,
    body_ptr: *const u8,
    body_len: usize,
    out_receipt: *mut RnsEmbeddedV1SendReceipt,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        if destination_ptr.is_null() || out_receipt.is_null() {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        }
        let Some(body) = byte_slice(body_ptr, body_len) else {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        };
        // SAFETY: `destination_ptr` is validated non-null above and must reference
        // exactly one 16-byte destination buffer for the duration of this call.
        let destination_slice = unsafe { core::slice::from_raw_parts(destination_ptr, 16) };
        let mut destination = [0_u8; 16];
        destination.copy_from_slice(destination_slice);
        match node.node.send(destination, body, SendOptions) {
            Ok(receipt) => {
                // SAFETY: `out_receipt` is validated non-null above and points to
                // writable caller storage for one mapped receipt.
                unsafe {
                    *out_receipt = map_v1_receipt(receipt);
                }
                clear_v1_node_error(out_node_error)
            }
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_broadcast(
    node: *mut RnsEmbeddedV1Node,
    destinations_ptr: *const u8,
    destination_count: usize,
    body_ptr: *const u8,
    body_len: usize,
    out_receipt: *mut RnsEmbeddedV1SendReceipt,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        if out_receipt.is_null() {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        }
        let Some(body) = byte_slice(body_ptr, body_len) else {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        };
        let destinations = match destination_list(destinations_ptr, destination_count) {
            Some(destinations) => destinations,
            None => {
                return set_v1_pointer_error(
                    out_node_error,
                    RnsEmbeddedV1NodeErrorCode::InvalidPointer,
                )
            }
        };
        match node.node.broadcast(body, BroadcastOptions { destinations }) {
            Ok(receipt) => {
                // SAFETY: `out_receipt` is validated non-null above and points to
                // writable caller storage for one mapped receipt.
                unsafe {
                    *out_receipt = map_v1_receipt(receipt);
                }
                clear_v1_node_error(out_node_error)
            }
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_set_log_level(
    node: *mut RnsEmbeddedV1Node,
    level: RnsEmbeddedV1LogLevel,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        match node.node.set_log_level(map_v1_log_level(level)) {
            Ok(()) => clear_v1_node_error(out_node_error),
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_subscribe_events(
    node: *mut RnsEmbeddedV1Node,
    out_subscription: *mut *mut RnsEmbeddedEventSubscription,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(node) = v1_node_mut(node) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        if out_subscription.is_null() {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        }
        match node.node.subscribe_events() {
            Ok(subscription) => {
                // SAFETY: `out_subscription` is validated non-null above and points
                // to writable caller storage for the newly allocated handle.
                unsafe {
                    *out_subscription =
                        Box::into_raw(Box::new(RnsEmbeddedEventSubscription { subscription }));
                }
                clear_v1_node_error(out_node_error)
            }
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_subscription_next(
    subscription: *mut RnsEmbeddedEventSubscription,
    timeout_ms: u64,
    out_poll_result: *mut RnsEmbeddedV1PollResult,
    out_event: *mut RnsEmbeddedV1NodeEvent,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        let Some(subscription) = v1_subscription_mut(subscription) else {
            return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
        };
        if out_poll_result.is_null() || out_event.is_null() {
            return set_v1_pointer_error(
                out_node_error,
                RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            );
        }
        match subscription.subscription.next(timeout_ms) {
            Ok(result) => {
                // SAFETY: both output pointers are validated non-null above and
                // point to writable caller storage for this poll result.
                unsafe {
                    *out_poll_result = map_v1_poll_result(&result);
                    *out_event = match result {
                        PollResult::Event(ref event) => map_v1_event(event),
                        _ => RnsEmbeddedV1NodeEvent::default(),
                    };
                }
                set_v1_poll_sideband_error(out_node_error, &result)
            }
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_subscription_close(
    subscription: *mut RnsEmbeddedEventSubscription,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    ffi_v1_node_error_boundary(out_node_error, || {
        if subscription.is_null() {
            return clear_v1_node_error(out_node_error);
        }
        // SAFETY: `subscription` comes from `Box::into_raw` in
        // `rns_embedded_v1_node_subscribe_events` and is reclaimed exactly once here.
        let boxed = unsafe { Box::from_raw(subscription) };
        match boxed.subscription.close() {
            Ok(()) => clear_v1_node_error(out_node_error),
            Err(err) => set_v1_node_error(out_node_error, err),
        }
    })
}

fn node_mut<'a>(node: *mut RnsEmbeddedNode) -> Option<&'a mut RnsEmbeddedNode> {
    if node.is_null() {
        return None;
    }
    // SAFETY: caller passes a pointer returned by `rns_embedded_node_new`; this
    // helper only creates a temporary exclusive borrow for the duration of the call.
    Some(unsafe { &mut *node })
}

fn v1_node_mut<'a>(node: *mut RnsEmbeddedV1Node) -> Option<&'a mut RnsEmbeddedV1Node> {
    if node.is_null() {
        return None;
    }
    // SAFETY: callers pass handles allocated by this crate and this helper only
    // creates a temporary exclusive borrow for the duration of the FFI call.
    Some(unsafe { &mut *node })
}

fn v1_subscription_mut<'a>(
    subscription: *mut RnsEmbeddedEventSubscription,
) -> Option<&'a mut RnsEmbeddedEventSubscription> {
    if subscription.is_null() {
        return None;
    }
    // SAFETY: callers pass handles allocated by this crate and this helper only
    // creates a temporary exclusive borrow for the duration of the FFI call.
    Some(unsafe { &mut *subscription })
}

fn byte_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller provides a non-null pointer to at least `len` readable bytes
    // that remain valid for the duration of the call.
    Some(unsafe { core::slice::from_raw_parts(ptr, len) })
}

fn destination_list(ptr: *const u8, count: usize) -> Option<alloc::vec::Vec<[u8; 16]>> {
    if count == 0 {
        return Some(alloc::vec::Vec::new());
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `ptr` is validated non-null above and the byte count is derived
    // from the caller-provided destination count with fixed 16-byte entries.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, count.saturating_mul(16)) };
    let mut out = alloc::vec::Vec::with_capacity(count);
    for chunk in bytes.chunks_exact(16) {
        let mut destination = [0_u8; 16];
        destination.copy_from_slice(chunk);
        out.push(destination);
    }
    Some(out)
}

fn v1_node_config(config: &RnsEmbeddedV1NodeConfig) -> Result<NodeConfig, NodeError> {
    if config.struct_size < core::mem::size_of::<RnsEmbeddedV1NodeConfig>() / 2 {
        return Err(NodeError::InvalidConfig);
    }
    let backend = match config.node_mode {
        RnsEmbeddedNodeMode::BleOnly => NodeBackendConfig::Ble(BleNodeBackendConfig {
            mtu_hint: config.ble_mtu_hint,
            max_inbound_frames: config.ble_max_inbound_frames,
            max_outbound_frames: config.ble_max_outbound_frames,
            ordered_delivery: config.ble_ordered_delivery,
        }),
        RnsEmbeddedNodeMode::TcpClient => NodeBackendConfig::TcpClient(TcpClientConfig {
            host: parse_c_string_bytes(&config.tcp_host)?,
            port: config.tcp_port,
            reconnect_backoff_ms: alloc::vec![250, 500, 1_000, 2_000],
        }),
        RnsEmbeddedNodeMode::TcpServer => {
            NodeBackendConfig::TcpServer(TcpServerConfig { listen_port: config.tcp_listen_port })
        }
    };
    Ok(NodeConfig {
        runtime: RuntimeConfig {
            store_identity: config.store_identity,
            lxmf_address: config.lxmf_address,
            node_mode: match config.node_mode {
                RnsEmbeddedNodeMode::BleOnly => NodeTransportMode::BleOnly,
                RnsEmbeddedNodeMode::TcpClient => NodeTransportMode::TcpClient,
                RnsEmbeddedNodeMode::TcpServer => NodeTransportMode::TcpServer,
            },
            announce_interval_ms: config.announce_interval_ms,
            max_outbound_queue: config.max_outbound_queue,
            max_events: config.max_events,
            capture_defaults: CaptureDefaults { max_bytes: config.capture_default_max_bytes },
        },
        backend,
    })
}

fn parse_c_string_bytes(bytes: &[u8]) -> Result<String, NodeError> {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    if end == 0 {
        return Err(NodeError::InvalidConfig);
    }
    core::str::from_utf8(&bytes[..end])
        .map(|value| value.to_string())
        .map_err(|_| NodeError::InvalidConfig)
}

fn map_v1_status(status: NodeStatus) -> RnsEmbeddedV1NodeStatus {
    RnsEmbeddedV1NodeStatus {
        struct_size: core::mem::size_of::<RnsEmbeddedV1NodeStatus>(),
        struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
        run_state: match status.run_state {
            NodeRunState::Stopped => RnsEmbeddedV1RunState::Stopped,
            NodeRunState::Running => RnsEmbeddedV1RunState::Running,
        },
        epoch: status.epoch,
        lifecycle_state: status
            .lifecycle_state
            .map(map_lifecycle_state)
            .unwrap_or(RnsEmbeddedLifecycleState::Boot),
        pending_outbound: status.pending_outbound,
        announces_queued: status.stats.announces_queued,
        outbound_sent: status.stats.outbound_sent,
        outbound_deferred: status.stats.outbound_deferred,
        inbound_accepted: status.stats.inbound_accepted,
        inbound_rejected: status.stats.inbound_rejected,
        announces_received: status.stats.announces_received,
        lxmf_messages_received: status.stats.lxmf_messages_received,
        log_level: map_log_level(status.log_level),
        reserved: [0; 24],
    }
}

fn map_v1_event(event: &NodeEvent) -> RnsEmbeddedV1NodeEvent {
    let mut out = RnsEmbeddedV1NodeEvent {
        struct_size: core::mem::size_of::<RnsEmbeddedV1NodeEvent>(),
        struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
        event_id: event.event_id,
        epoch: event.epoch,
        occurred_at_ms: event.occurred_at_ms,
        operation_id: event.operation_id.unwrap_or_default(),
        has_operation_id: event.operation_id.is_some(),
        ..RnsEmbeddedV1NodeEvent::default()
    };
    match &event.kind {
        NodeEventKind::StatusChanged { run_state, lifecycle_state } => {
            out.kind = RnsEmbeddedV1EventKind::StatusChanged;
            out.run_state = match run_state {
                NodeRunState::Stopped => RnsEmbeddedV1RunState::Stopped,
                NodeRunState::Running => RnsEmbeddedV1RunState::Running,
            };
            out.lifecycle_state = (*lifecycle_state)
                .map(map_lifecycle_state)
                .unwrap_or(RnsEmbeddedLifecycleState::Boot);
        }
        NodeEventKind::Log { level, code } => {
            out.kind = RnsEmbeddedV1EventKind::Log;
            out.log_level = map_log_level(*level);
            out.value0 = u64::from(*code);
        }
        NodeEventKind::Error { error, frame_kind, sequence } => {
            out.kind = RnsEmbeddedV1EventKind::Error;
            out.error_code = map_v1_node_error_code(error);
            out.frame_kind = *frame_kind;
            out.sequence = *sequence;
        }
        NodeEventKind::PacketReceived { frame_kind, sequence, bytes } => {
            out.kind = RnsEmbeddedV1EventKind::PacketReceived;
            out.frame_kind = *frame_kind;
            out.sequence = *sequence;
            out.bytes = *bytes;
        }
        NodeEventKind::PacketSent { frame_kind, sequence, bytes } => {
            out.kind = RnsEmbeddedV1EventKind::PacketSent;
            out.frame_kind = *frame_kind;
            out.sequence = *sequence;
            out.bytes = *bytes;
        }
        NodeEventKind::Extension { extension_id, value0, value1 } => {
            if rns_embedded_runtime::node::is_valid_extension_id(*extension_id) {
                out.kind = RnsEmbeddedV1EventKind::Extension;
                out.extension_id = *extension_id;
                out.value0 = *value0;
                out.value1 = *value1;
            } else {
                out.kind = RnsEmbeddedV1EventKind::Error;
                out.error_code = RnsEmbeddedV1NodeErrorCode::InternalError;
            }
        }
    }
    out
}

fn map_v1_poll_result(result: &PollResult) -> RnsEmbeddedV1PollResult {
    let mut out = RnsEmbeddedV1PollResult::default();
    match result {
        PollResult::Event(event) => {
            out.kind = RnsEmbeddedV1PollResultKind::Event;
            out.epoch = event.epoch;
        }
        PollResult::Timeout => out.kind = RnsEmbeddedV1PollResultKind::Timeout,
        PollResult::Closed => out.kind = RnsEmbeddedV1PollResultKind::Closed,
        PollResult::Gap { next_event_id } => {
            out.kind = RnsEmbeddedV1PollResultKind::Gap;
            out.next_event_id = *next_event_id;
        }
        PollResult::NodeStopped => out.kind = RnsEmbeddedV1PollResultKind::NodeStopped,
        PollResult::NodeRestarted { epoch } => {
            out.kind = RnsEmbeddedV1PollResultKind::NodeRestarted;
            out.epoch = *epoch;
        }
    }
    out
}

fn map_v1_receipt(receipt: rns_embedded_runtime::NodeOperationReceipt) -> RnsEmbeddedV1SendReceipt {
    RnsEmbeddedV1SendReceipt {
        struct_size: core::mem::size_of::<RnsEmbeddedV1SendReceipt>(),
        struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
        operation_id: receipt.operation_id,
        epoch: receipt.epoch,
        accepted_bytes: receipt.accepted_bytes,
        queued: receipt.queued,
        target_count: receipt.target_count,
        reserved: [0; 24],
    }
}

fn map_lifecycle_state(state: NodeLifecycleState) -> RnsEmbeddedLifecycleState {
    match state {
        NodeLifecycleState::Boot => RnsEmbeddedLifecycleState::Boot,
        NodeLifecycleState::Unprovisioned => RnsEmbeddedLifecycleState::Unprovisioned,
        NodeLifecycleState::ProvisionedOffline => RnsEmbeddedLifecycleState::ProvisionedOffline,
        NodeLifecycleState::TcpOnline => RnsEmbeddedLifecycleState::TcpOnline,
        NodeLifecycleState::BleRecovery => RnsEmbeddedLifecycleState::BleRecovery,
        NodeLifecycleState::FailureReconnect => RnsEmbeddedLifecycleState::FailureReconnect,
    }
}

fn map_log_level(level: NodeLogLevel) -> RnsEmbeddedV1LogLevel {
    match level {
        NodeLogLevel::Error => RnsEmbeddedV1LogLevel::Error,
        NodeLogLevel::Warn => RnsEmbeddedV1LogLevel::Warn,
        NodeLogLevel::Info => RnsEmbeddedV1LogLevel::Info,
        NodeLogLevel::Debug => RnsEmbeddedV1LogLevel::Debug,
        NodeLogLevel::Trace => RnsEmbeddedV1LogLevel::Trace,
    }
}

fn map_v1_log_level(level: RnsEmbeddedV1LogLevel) -> NodeLogLevel {
    match level {
        RnsEmbeddedV1LogLevel::Error => NodeLogLevel::Error,
        RnsEmbeddedV1LogLevel::Warn => NodeLogLevel::Warn,
        RnsEmbeddedV1LogLevel::Info => NodeLogLevel::Info,
        RnsEmbeddedV1LogLevel::Debug => NodeLogLevel::Debug,
        RnsEmbeddedV1LogLevel::Trace => NodeLogLevel::Trace,
    }
}

fn ffi_status_boundary<F>(f: F) -> RnsEmbeddedStatus
where
    F: FnOnce() -> RnsEmbeddedStatus,
{
    #[cfg(feature = "std")]
    {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(RnsEmbeddedStatus::InvalidState)
    }

    #[cfg(not(feature = "std"))]
    {
        f()
    }
}

fn ffi_ptr_boundary<T, F>(f: F) -> *mut T
where
    F: FnOnce() -> *mut T,
{
    #[cfg(feature = "std")]
    {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(core::ptr::null_mut())
    }

    #[cfg(not(feature = "std"))]
    {
        f()
    }
}

fn ffi_v1_node_error_boundary<F>(
    out_node_error: *mut RnsEmbeddedV1NodeError,
    f: F,
) -> RnsEmbeddedStatus
where
    F: FnOnce() -> RnsEmbeddedStatus,
{
    #[cfg(feature = "std")]
    {
        catch_unwind(AssertUnwindSafe(f))
            .unwrap_or_else(|_| set_v1_node_error(out_node_error, NodeError::InternalError))
    }

    #[cfg(not(feature = "std"))]
    {
        f()
    }
}

fn clear_v1_node_error(out_node_error: *mut RnsEmbeddedV1NodeError) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
        // SAFETY: `out_node_error` is checked non-null above and points to
        // writable caller storage for one sideband error struct.
        unsafe {
            *out_node_error = RnsEmbeddedV1NodeError::default();
        }
    }
    RnsEmbeddedStatus::Ok
}

fn set_v1_poll_sideband_error(
    out_node_error: *mut RnsEmbeddedV1NodeError,
    result: &PollResult,
) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
        let code = match result {
            PollResult::Closed => RnsEmbeddedV1NodeErrorCode::SubscriptionClosed,
            PollResult::Gap { .. } => RnsEmbeddedV1NodeErrorCode::EventGap,
            PollResult::NodeRestarted { .. } => RnsEmbeddedV1NodeErrorCode::NodeRestarted,
            PollResult::Timeout => RnsEmbeddedV1NodeErrorCode::Timeout,
            PollResult::NodeStopped => RnsEmbeddedV1NodeErrorCode::NotRunning,
            PollResult::Event(_) => RnsEmbeddedV1NodeErrorCode::Unknown,
        };
        // SAFETY: `out_node_error` is checked non-null above and points to
        // writable caller storage for one sideband error struct.
        unsafe {
            *out_node_error = RnsEmbeddedV1NodeError { code, ..RnsEmbeddedV1NodeError::default() };
        }
    }
    RnsEmbeddedStatus::Ok
}

fn set_v1_pointer_error(
    out_node_error: *mut RnsEmbeddedV1NodeError,
    code: RnsEmbeddedV1NodeErrorCode,
) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
        // SAFETY: `out_node_error` is checked non-null above and points to
        // writable caller storage for one sideband error struct.
        unsafe {
            *out_node_error = RnsEmbeddedV1NodeError { code, ..RnsEmbeddedV1NodeError::default() };
        }
    }
    RnsEmbeddedStatus::InvalidArgument
}

fn set_v1_node_error(
    out_node_error: *mut RnsEmbeddedV1NodeError,
    error: NodeError,
) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
        // SAFETY: `out_node_error` is checked non-null above and points to
        // writable caller storage for one sideband error struct.
        unsafe {
            *out_node_error = RnsEmbeddedV1NodeError {
                code: map_v1_node_error_code(&error),
                ..RnsEmbeddedV1NodeError::default()
            };
        }
    }
    map_node_error_status(error)
}

fn map_v1_node_error_code(error: &NodeError) -> RnsEmbeddedV1NodeErrorCode {
    match error {
        NodeError::InvalidConfig => RnsEmbeddedV1NodeErrorCode::InvalidConfig,
        NodeError::IoError => RnsEmbeddedV1NodeErrorCode::IoError,
        NodeError::NetworkError => RnsEmbeddedV1NodeErrorCode::NetworkError,
        NodeError::ReticulumError => RnsEmbeddedV1NodeErrorCode::ReticulumError,
        NodeError::AlreadyRunning => RnsEmbeddedV1NodeErrorCode::AlreadyRunning,
        NodeError::NotRunning => RnsEmbeddedV1NodeErrorCode::NotRunning,
        NodeError::Timeout => RnsEmbeddedV1NodeErrorCode::Timeout,
        NodeError::InternalError => RnsEmbeddedV1NodeErrorCode::InternalError,
        NodeError::ModeConflict => RnsEmbeddedV1NodeErrorCode::ModeConflict,
        NodeError::SubscriptionClosed => RnsEmbeddedV1NodeErrorCode::SubscriptionClosed,
        NodeError::NodeRestarted => RnsEmbeddedV1NodeErrorCode::NodeRestarted,
        NodeError::EventGap => RnsEmbeddedV1NodeErrorCode::EventGap,
        NodeError::QueuePressure => RnsEmbeddedV1NodeErrorCode::QueuePressure,
    }
}

fn map_node_error_status(error: NodeError) -> RnsEmbeddedStatus {
    match error {
        NodeError::InvalidConfig => RnsEmbeddedStatus::InvalidInput,
        NodeError::IoError => RnsEmbeddedStatus::InvalidState,
        NodeError::NetworkError => RnsEmbeddedStatus::Disconnected,
        NodeError::ReticulumError => RnsEmbeddedStatus::InvalidState,
        NodeError::AlreadyRunning
        | NodeError::NotRunning
        | NodeError::InternalError
        | NodeError::ModeConflict => RnsEmbeddedStatus::InvalidState,
        NodeError::Timeout => RnsEmbeddedStatus::Timeout,
        NodeError::SubscriptionClosed | NodeError::NodeRestarted | NodeError::EventGap => {
            RnsEmbeddedStatus::Ok
        }
        NodeError::QueuePressure => RnsEmbeddedStatus::Backpressure,
    }
}

fn map_status(result: Result<(), EmbeddedError>) -> RnsEmbeddedStatus {
    match result {
        Ok(()) => RnsEmbeddedStatus::Ok,
        Err(err) => map_embedded_error(err),
    }
}

fn map_embedded_error(error: EmbeddedError) -> RnsEmbeddedStatus {
    match error {
        EmbeddedError::InvalidInput => RnsEmbeddedStatus::InvalidInput,
        EmbeddedError::InvalidArgument => RnsEmbeddedStatus::InvalidArgument,
        EmbeddedError::InvalidCursor | EmbeddedError::InvalidState => {
            RnsEmbeddedStatus::InvalidState
        }
        EmbeddedError::NotFound => RnsEmbeddedStatus::NotFound,
        EmbeddedError::SeqGap => RnsEmbeddedStatus::SeqGap,
        EmbeddedError::IntegrityFailure => RnsEmbeddedStatus::IntegrityFailure,
        EmbeddedError::ChecksumMismatch => RnsEmbeddedStatus::ChecksumMismatch,
        EmbeddedError::IdempotencyConflict => RnsEmbeddedStatus::IdempotencyConflict,
        EmbeddedError::ReplayRejected => RnsEmbeddedStatus::ReplayRejected,
        EmbeddedError::Timeout => RnsEmbeddedStatus::Timeout,
        EmbeddedError::Backpressure => RnsEmbeddedStatus::Backpressure,
        EmbeddedError::Disconnected => RnsEmbeddedStatus::Disconnected,
        EmbeddedError::StorageCorruption => RnsEmbeddedStatus::StorageCorruption,
        EmbeddedError::Unsupported => RnsEmbeddedStatus::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::Value;

    use super::{
        ffi_v1_node_error_boundary, rns_embedded_node_free, rns_embedded_node_new,
        rns_embedded_node_push_inbound_wire, rns_embedded_node_queue_message,
        rns_embedded_node_set_link_state, rns_embedded_node_take_outbound_wire,
        rns_embedded_node_tick, rns_embedded_v1_abi_version, rns_embedded_v1_get_capabilities,
        rns_embedded_v1_node_broadcast, rns_embedded_v1_node_config_default,
        rns_embedded_v1_node_free, rns_embedded_v1_node_get_status, rns_embedded_v1_node_new,
        rns_embedded_v1_node_restart, rns_embedded_v1_node_send,
        rns_embedded_v1_node_set_log_level, rns_embedded_v1_node_start, rns_embedded_v1_node_stop,
        rns_embedded_v1_node_subscribe_events, rns_embedded_v1_subscription_close,
        rns_embedded_v1_subscription_next, RnsEmbeddedLinkState, RnsEmbeddedNodeConfig,
        RnsEmbeddedStatus, RnsEmbeddedV1Capabilities, RnsEmbeddedV1EventKind,
        RnsEmbeddedV1LogLevel, RnsEmbeddedV1NodeError, RnsEmbeddedV1NodeErrorCode,
        RnsEmbeddedV1NodeEvent, RnsEmbeddedV1NodeStatus, RnsEmbeddedV1PollResult,
        RnsEmbeddedV1PollResultKind, RnsEmbeddedV1RunState, RnsEmbeddedV1SendReceipt,
        RNS_EMBEDDED_V1_CAPABILITY_SCHEMA_VERSION, RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT,
        RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST, RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI,
        RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING, RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
        RNS_EMBEDDED_V1_DRIVER_TICK_MAX_MS, RNS_EMBEDDED_V1_DRIVER_TICK_TARGET_MS,
        RNS_EMBEDDED_V1_KNOWN_CAPABILITY_BITS, RNS_EMBEDDED_V1_MAX_BLOCKING_TIMEOUT_MS,
    };
    use rns_embedded_core::packet::{decode_frame, encode_frame, PacketFrame};
    use rns_embedded_runtime::node::{
        is_valid_extension_id, NODE_EXTENSION_ID_BOOTSTRAPPED, NODE_EXTENSION_ID_MESSAGE_QUEUED,
        NODE_EXTENSION_ID_RECEIVED_SUMMARY,
    };

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/fixtures/embedded/public-node-api-v1")
            .join(name)
    }

    fn contract_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/contracts").join(name)
    }

    fn json_fixture(name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(fixture_path(name)).expect("read fixture"))
            .expect("parse fixture")
    }

    fn contract_json(name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(contract_path(name)).expect("read contract"))
            .expect("parse contract")
    }

    fn capability_bit(name: &str) -> u64 {
        match name {
            "RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME" => RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
            "RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT" => RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT,
            "RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST" => {
                RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST
            }
            "RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI" => RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI,
            "RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING" => RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING,
            other => panic!("unknown capability bit {other}"),
        }
    }

    fn node_error_code(name: &str) -> RnsEmbeddedV1NodeErrorCode {
        match name {
            "Unknown" => RnsEmbeddedV1NodeErrorCode::Unknown,
            "InvalidConfig" => RnsEmbeddedV1NodeErrorCode::InvalidConfig,
            "IoError" => RnsEmbeddedV1NodeErrorCode::IoError,
            "NetworkError" => RnsEmbeddedV1NodeErrorCode::NetworkError,
            "ReticulumError" => RnsEmbeddedV1NodeErrorCode::ReticulumError,
            "AlreadyRunning" => RnsEmbeddedV1NodeErrorCode::AlreadyRunning,
            "NotRunning" => RnsEmbeddedV1NodeErrorCode::NotRunning,
            "Timeout" => RnsEmbeddedV1NodeErrorCode::Timeout,
            "InternalError" => RnsEmbeddedV1NodeErrorCode::InternalError,
            "InvalidHandle" => RnsEmbeddedV1NodeErrorCode::InvalidHandle,
            "InvalidPointer" => RnsEmbeddedV1NodeErrorCode::InvalidPointer,
            "ModeConflict" => RnsEmbeddedV1NodeErrorCode::ModeConflict,
            "SubscriptionClosed" => RnsEmbeddedV1NodeErrorCode::SubscriptionClosed,
            "NodeRestarted" => RnsEmbeddedV1NodeErrorCode::NodeRestarted,
            "EventGap" => RnsEmbeddedV1NodeErrorCode::EventGap,
            "QueuePressure" => RnsEmbeddedV1NodeErrorCode::QueuePressure,
            other => panic!("unknown node error code {other}"),
        }
    }

    fn poll_kind(name: &str) -> RnsEmbeddedV1PollResultKind {
        match name {
            "Event" => RnsEmbeddedV1PollResultKind::Event,
            "Timeout" => RnsEmbeddedV1PollResultKind::Timeout,
            "Closed" => RnsEmbeddedV1PollResultKind::Closed,
            "Gap" => RnsEmbeddedV1PollResultKind::Gap,
            "NodeStopped" => RnsEmbeddedV1PollResultKind::NodeStopped,
            "NodeRestarted" => RnsEmbeddedV1PollResultKind::NodeRestarted,
            other => panic!("unknown poll kind {other}"),
        }
    }

    fn status_code(name: &str) -> RnsEmbeddedStatus {
        match name {
            "Ok" => RnsEmbeddedStatus::Ok,
            "Backpressure" => RnsEmbeddedStatus::Backpressure,
            other => panic!("unknown status {other}"),
        }
    }

    #[test]
    fn ffi_node_ticks_and_drains_outbound_wire() {
        let config = RnsEmbeddedNodeConfig::default();
        let node = rns_embedded_node_new(&config);
        assert!(!node.is_null());

        assert_eq!(
            rns_embedded_node_set_link_state(node, RnsEmbeddedLinkState::Up),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(rns_embedded_node_tick(node, 0), RnsEmbeddedStatus::Ok);

        let mut out = [0_u8; 256];
        let mut out_len = 0usize;
        assert_eq!(
            rns_embedded_node_take_outbound_wire(node, out.as_mut_ptr(), out.len(), &mut out_len),
            RnsEmbeddedStatus::Ok
        );
        let frame = decode_frame(&out[..out_len]).expect("decode frame");
        assert_eq!(frame.kind, 0x11);

        rns_embedded_node_free(node);
    }

    #[test]
    fn ffi_node_accepts_inbound_and_queues_message() {
        let config = RnsEmbeddedNodeConfig::default();
        let node = rns_embedded_node_new(&config);
        assert!(!node.is_null());

        assert_eq!(
            rns_embedded_node_set_link_state(node, RnsEmbeddedLinkState::Up),
            RnsEmbeddedStatus::Ok
        );

        let inbound = encode_frame(&PacketFrame::new(0x44, 9, b"ping".to_vec()).expect("frame"))
            .expect("encode");
        assert_eq!(
            rns_embedded_node_push_inbound_wire(node, inbound.as_ptr(), inbound.len()),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(rns_embedded_node_tick(node, 0), RnsEmbeddedStatus::Ok);

        let destination = [0x7A_u8; 16];
        let mut sequence = 0_u32;
        assert_eq!(
            rns_embedded_node_queue_message(
                node,
                destination.as_ptr(),
                b"hello".as_ptr(),
                b"hello".len(),
                &mut sequence,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert!(sequence > 0);

        rns_embedded_node_free(node);
    }

    #[test]
    fn ffi_v1_reports_capabilities_and_status() {
        assert_eq!(rns_embedded_v1_abi_version(), 1);
        let fixture = json_fixture("capability-probe.json");

        let mut capabilities = RnsEmbeddedV1Capabilities::default();
        assert_eq!(rns_embedded_v1_get_capabilities(&mut capabilities), RnsEmbeddedStatus::Ok);
        assert_eq!(capabilities.abi_version, 1);
        assert_eq!(
            capabilities.capability_schema_version,
            fixture["capability_schema_version"].as_u64().expect("schema version") as u32
        );
        assert_eq!(
            capabilities.capability_schema_version,
            RNS_EMBEDDED_V1_CAPABILITY_SCHEMA_VERSION
        );
        assert_eq!(capabilities.known_capability_bits, RNS_EMBEDDED_V1_KNOWN_CAPABILITY_BITS);
        assert_eq!(
            capabilities.known_capability_bits,
            fixture["known_capability_bits"].as_u64().expect("known bits")
        );
        assert_eq!(capabilities.capability_bits, capabilities.compile_time_capability_bits);
        assert_ne!(capabilities.capability_bits, 0);
        assert_eq!(fixture["unknown_bit_policy"].as_str().expect("policy"), "ignore");
        assert_eq!(
            (capabilities.capability_bits | (1_u64 << 63)) & capabilities.known_capability_bits,
            capabilities.capability_bits
        );
        assert_ne!(capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST, 0);
        assert_ne!(capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI, 0);
        assert_ne!(capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING, 0);
        for name in
            fixture["compile_time_required_bits"].as_array().expect("compile time required bits")
        {
            let bit = capability_bit(name.as_str().expect("bit name"));
            assert_ne!(capabilities.compile_time_capability_bits & bit, 0);
            assert_ne!(capabilities.capability_bits & bit, 0);
        }
        assert_eq!(
            capabilities.max_subscriptions,
            fixture["max_subscriptions"].as_u64().expect("max subscriptions") as u32
        );

        #[cfg(feature = "std")]
        {
            for name in fixture["std_required_bits"].as_array().expect("std bits") {
                let bit = capability_bit(name.as_str().expect("bit"));
                assert_ne!(capabilities.capability_bits & bit, 0);
            }
            assert_eq!(
                capabilities.max_blocking_timeout_ms,
                RNS_EMBEDDED_V1_MAX_BLOCKING_TIMEOUT_MS
            );
            assert_eq!(capabilities.driver_tick_target_ms, RNS_EMBEDDED_V1_DRIVER_TICK_TARGET_MS);
            assert_eq!(capabilities.driver_tick_max_ms, RNS_EMBEDDED_V1_DRIVER_TICK_MAX_MS);
        }

        #[cfg(not(feature = "std"))]
        {
            for name in fixture["alloc_forbidden_bits"].as_array().expect("alloc forbidden bits") {
                let bit = capability_bit(name.as_str().expect("bit"));
                assert_eq!(capabilities.capability_bits & bit, 0);
            }
            assert_eq!(capabilities.max_blocking_timeout_ms, 0);
            assert_eq!(capabilities.driver_tick_target_ms, 0);
            assert_eq!(capabilities.driver_tick_max_ms, 0);
        }

        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let mut config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::Unknown);

        let mut status = RnsEmbeddedV1NodeStatus::default();
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.run_state, RnsEmbeddedV1RunState::Running);
        assert_eq!(status.epoch, 1);

        config.announce_interval_ms = 2_000;
        assert_eq!(rns_embedded_v1_node_restart(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.epoch, 2);

        assert_eq!(rns_embedded_v1_node_stop(node, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.run_state, RnsEmbeddedV1RunState::Stopped);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_send_and_broadcast_surface_node_errors() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);

        assert_eq!(
            rns_embedded_v1_node_set_log_level(node, RnsEmbeddedV1LogLevel::Debug, &mut error),
            RnsEmbeddedStatus::Ok
        );

        let destination = [0xA5_u8; 16];
        let mut receipt = RnsEmbeddedV1SendReceipt::default();
        assert_eq!(
            rns_embedded_v1_node_send(
                node,
                destination.as_ptr(),
                b"hello".as_ptr(),
                b"hello".len(),
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(receipt.target_count, 1);
        assert_eq!(receipt.epoch, 1);

        let destinations = [[0x10_u8; 16], [0x20_u8; 16]];
        assert_eq!(
            rns_embedded_v1_node_broadcast(
                node,
                destinations.as_ptr().cast::<u8>(),
                destinations.len(),
                b"fanout".as_ptr(),
                b"fanout".len(),
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(receipt.target_count, 2);

        assert_eq!(
            rns_embedded_v1_node_broadcast(
                node,
                core::ptr::null(),
                0,
                b"fanout".as_ptr(),
                b"fanout".len(),
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::InvalidInput
        );
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::InvalidConfig);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_legacy_truncation_reports_required_length() {
        let fixture = json_fixture("truncation-reporting.json");
        let config = RnsEmbeddedNodeConfig::default();
        let node = rns_embedded_node_new(&config);
        assert!(!node.is_null());

        assert_eq!(
            rns_embedded_node_set_link_state(node, RnsEmbeddedLinkState::Up),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(rns_embedded_node_tick(node, 0), RnsEmbeddedStatus::Ok);

        let mut out = [0_u8; 1];
        let mut out_len = 0usize;
        assert_eq!(
            rns_embedded_node_take_outbound_wire(
                node,
                out.as_mut_ptr(),
                fixture["small_buffer_len"].as_u64().expect("small buffer len") as usize,
                &mut out_len,
            ),
            status_code(fixture["expected_status"].as_str().expect("status"))
        );
        assert!(out_len >= fixture["required_len_min"].as_u64().expect("required len") as usize);

        rns_embedded_node_free(node);
    }

    #[test]
    fn ffi_v1_queue_pressure_maps_to_stable_error_code() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let mut config = rns_embedded_v1_node_config_default();
        config.max_outbound_queue = 1;
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut receipt = RnsEmbeddedV1SendReceipt::default();
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);

        let destination = [0x42_u8; 16];
        assert_eq!(
            rns_embedded_v1_node_send(
                node,
                destination.as_ptr(),
                b"one".as_ptr(),
                3,
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_node_send(
                node,
                destination.as_ptr(),
                b"two".as_ptr(),
                3,
                &mut receipt,
                &mut error,
            ),
            RnsEmbeddedStatus::Backpressure
        );
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::QueuePressure);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_subscriptions_surface_restart_and_status_events() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut subscription = core::ptr::null_mut();
        assert_eq!(
            rns_embedded_v1_node_subscribe_events(node, &mut subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert!(!subscription.is_null());

        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);

        let mut poll = RnsEmbeddedV1PollResult::default();
        let mut event = RnsEmbeddedV1NodeEvent::default();
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::NodeRestarted);
        assert_eq!(poll.epoch, 1);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::NodeRestarted);

        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::Event);
        assert_eq!(event.kind, RnsEmbeddedV1EventKind::StatusChanged);
        assert_eq!(event.epoch, 1);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::Unknown);

        assert_eq!(rns_embedded_v1_node_stop(node, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::NodeStopped);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::NotRunning);

        assert_eq!(
            rns_embedded_v1_subscription_close(subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_timeout_and_gap_signaling_match_fixtures() {
        let timeout_fixture = json_fixture("poll-timeout.json");
        let gap_fixture = json_fixture("gap-restart-signaling.json");

        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut subscription = core::ptr::null_mut();
        assert_eq!(
            rns_embedded_v1_node_subscribe_events(node, &mut subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );

        let mut poll = RnsEmbeddedV1PollResult::default();
        let mut event = RnsEmbeddedV1NodeEvent::default();
        assert_eq!(
            rns_embedded_v1_subscription_next(
                subscription,
                timeout_fixture["timeout_ms"].as_u64().expect("timeout"),
                &mut poll,
                &mut event,
                &mut error,
            ),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            poll.kind,
            poll_kind(timeout_fixture["expected_poll_kind"].as_str().expect("timeout poll kind"))
        );
        assert_eq!(
            error.code,
            node_error_code(timeout_fixture["expected_error_code"].as_str().expect("timeout code"))
        );

        let mut config = rns_embedded_v1_node_config_default();
        config.max_events = 1;
        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            poll.kind,
            poll_kind(gap_fixture["expected_restart_poll_kind"].as_str().expect("restart kind"))
        );
        assert_eq!(
            error.code,
            node_error_code(
                gap_fixture["expected_restart_error_code"].as_str().expect("restart code")
            )
        );

        assert_eq!(
            rns_embedded_v1_node_set_log_level(node, RnsEmbeddedV1LogLevel::Debug, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_node_set_log_level(node, RnsEmbeddedV1LogLevel::Trace, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 0, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            poll.kind,
            poll_kind(gap_fixture["expected_gap_poll_kind"].as_str().expect("gap kind"))
        );
        assert_eq!(
            error.code,
            node_error_code(gap_fixture["expected_gap_error_code"].as_str().expect("gap code"))
        );

        assert_eq!(
            rns_embedded_v1_subscription_close(subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );
        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_restart_epoch_matches_fixture() {
        let fixture = json_fixture("restart-epoch.json");
        let node = rns_embedded_v1_node_new();
        let mut error = RnsEmbeddedV1NodeError::default();
        let mut status = RnsEmbeddedV1NodeStatus::default();
        let config = rns_embedded_v1_node_config_default();

        assert_eq!(rns_embedded_v1_node_start(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.epoch, fixture["initial_epoch"].as_u64().expect("initial epoch"));

        assert_eq!(rns_embedded_v1_node_restart(node, &config, &mut error), RnsEmbeddedStatus::Ok);
        assert_eq!(rns_embedded_v1_node_get_status(node, &mut status), RnsEmbeddedStatus::Ok);
        assert_eq!(status.epoch, fixture["restarted_epoch"].as_u64().expect("restarted epoch"));

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_boundary_maps_panic_to_internal_error() {
        let mut error = RnsEmbeddedV1NodeError::default();

        let status = ffi_v1_node_error_boundary(&mut error, || -> RnsEmbeddedStatus {
            panic!("simulated ffi boundary panic");
        });

        assert_eq!(status, RnsEmbeddedStatus::InvalidState);
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::InternalError);
    }

    #[test]
    fn error_code_registry_matches_contract_artifact() {
        let artifact = contract_json("node-error-codes-v1.json");
        assert_eq!(artifact["schema_version"].as_u64().expect("schema version"), 1);

        for code in artifact["codes"].as_array().expect("error codes") {
            let variant = code["rust_variant"].as_str().expect("variant");
            let value = code["value"].as_u64().expect("value") as u32;
            assert_eq!(node_error_code(variant) as u32, value, "{variant}");
        }
    }

    #[test]
    fn extension_ids_follow_registry_fixture() {
        let fixture = json_fixture("extension-ids.json");
        let expected = [
            NODE_EXTENSION_ID_BOOTSTRAPPED,
            NODE_EXTENSION_ID_MESSAGE_QUEUED,
            NODE_EXTENSION_ID_RECEIVED_SUMMARY,
        ];

        for (index, entry) in fixture.as_array().expect("extension ids").iter().enumerate() {
            let numeric_id = entry["numeric_id"].as_u64().expect("numeric id") as u32;
            let registry_id = entry["registry_id"].as_str().expect("registry id");
            assert_eq!(numeric_id, expected[index]);
            assert!(is_valid_extension_id(numeric_id));
            assert!(registry_id.starts_with("event."));
            assert!(registry_id.ends_with(".v1"));
            assert!(registry_id.split('.').count() >= 4);
        }
        assert!(!is_valid_extension_id(99));
    }
}
