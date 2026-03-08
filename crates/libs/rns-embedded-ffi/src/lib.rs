#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

use alloc::boxed::Box;
use rns_embedded_core::{EmbeddedError, store::JournaledEmbeddedStore, transport::LinkState};
use rns_embedded_runtime::{
    BleNodeBackendConfig, BroadcastOptions, EmbeddedNode, EmbeddedNodeRuntime, EventSubscription,
    NodeBackendConfig, NodeConfig, NodeError, NodeEvent, NodeEventKind, NodeLogLevel, NodeRunState,
    NodeStatus, PollResult, RuntimeConfig, SendOptions, ble::{BleShimConfig, BleShimTransport},
    node::{CaptureDefaults, NodeLifecycleState, NodeTransportMode},
};

#[cfg(not(feature = "std"))]
use core::panic::PanicInfo;

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
unsafe impl critical_section::Impl for NoopCriticalSection {
    unsafe fn acquire() -> RawRestoreState {}

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
const RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME: u64 = 1 << 0;
const RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT: u64 = 1 << 1;
const RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST: u64 = 1 << 2;
const RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI: u64 = 1 << 3;
const RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING: u64 = 1 << 4;

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RnsEmbeddedV1NodeErrorCode {
    Unknown = 0,
    InvalidConfig = 1,
    IoError = 2,
    NetworkError = 3,
    ReticulumError = 4,
    AlreadyRunning = 5,
    NotRunning = 6,
    Timeout = 7,
    InternalError = 8,
    InvalidHandle = 9,
    InvalidPointer = 10,
}

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
    pub reserved: [u8; 32],
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
            reserved: [0; 32],
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
    pub capability_bits: u64,
    pub max_event_payload_bytes: u32,
    pub max_subscriptions: u32,
    pub reserved: [u8; 32],
}

impl Default for RnsEmbeddedV1Capabilities {
    fn default() -> Self {
        let mut capability_bits =
            RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST | RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI;
        if cfg!(feature = "std") {
            capability_bits |=
                RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME | RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT;
        }
        if cfg!(feature = "std") || cfg!(feature = "alloc") {
            capability_bits |= RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING;
        }
        Self {
            struct_size: core::mem::size_of::<Self>(),
            struct_version: RNS_EMBEDDED_V1_STRUCT_VERSION,
            abi_version: RNS_EMBEDDED_V1_ABI_VERSION,
            capability_bits,
            max_event_payload_bytes: 0,
            max_subscriptions: 1024,
            reserved: [0; 32],
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
    if out_capabilities.is_null() {
        return RnsEmbeddedStatus::InvalidArgument;
    }
    unsafe {
        *out_capabilities = RnsEmbeddedV1Capabilities::default();
    }
    RnsEmbeddedStatus::Ok
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_new() -> *mut RnsEmbeddedV1Node {
    ensure_allocator_ready();
    Box::into_raw(Box::new(RnsEmbeddedV1Node {
        node: EmbeddedNode::new(),
    }))
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_free(node: *mut RnsEmbeddedV1Node) {
    if node.is_null() {
        return;
    }
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
        capture_defaults: CaptureDefaults {
            max_bytes: config.capture_default_max_bytes,
        },
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

    let node = RnsEmbeddedNode {
        runtime,
        store: JournaledEmbeddedStore::new(),
        transport,
    };
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
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    if config.is_null() {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    }
    let config = unsafe { &*config };
    let node_config = match v1_node_config(config) {
        Ok(config) => config,
        Err(err) => return set_v1_node_error(out_node_error, err),
    };
    match node.node.start(node_config) {
        Ok(()) => clear_v1_node_error(out_node_error),
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_stop(
    node: *mut RnsEmbeddedV1Node,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    match node.node.stop() {
        Ok(()) => clear_v1_node_error(out_node_error),
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_restart(
    node: *mut RnsEmbeddedV1Node,
    config: *const RnsEmbeddedV1NodeConfig,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    if config.is_null() {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    }
    let config = unsafe { &*config };
    let node_config = match v1_node_config(config) {
        Ok(config) => config,
        Err(err) => return set_v1_node_error(out_node_error, err),
    };
    match node.node.restart(node_config) {
        Ok(()) => clear_v1_node_error(out_node_error),
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_get_status(
    node: *mut RnsEmbeddedV1Node,
    out_status: *mut RnsEmbeddedV1NodeStatus,
) -> RnsEmbeddedStatus {
    let Some(node) = v1_node_mut(node) else {
        return RnsEmbeddedStatus::InvalidArgument;
    };
    if out_status.is_null() {
        return RnsEmbeddedStatus::InvalidArgument;
    }
    let status = node.node.get_status();
    unsafe {
        *out_status = map_v1_status(status);
    }
    RnsEmbeddedStatus::Ok
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
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    if destination_ptr.is_null() || out_receipt.is_null() {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    }
    let Some(body) = byte_slice(body_ptr, body_len) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    };
    let destination_slice = unsafe { core::slice::from_raw_parts(destination_ptr, 16) };
    let mut destination = [0_u8; 16];
    destination.copy_from_slice(destination_slice);
    match node.node.send(destination, body, SendOptions) {
        Ok(receipt) => {
            unsafe {
                *out_receipt = map_v1_receipt(receipt);
            }
            clear_v1_node_error(out_node_error)
        }
        Err(err) => set_v1_node_error(out_node_error, err),
    }
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
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    if out_receipt.is_null() {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    }
    let Some(body) = byte_slice(body_ptr, body_len) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    };
    let destinations = match destination_list(destinations_ptr, destination_count) {
        Some(destinations) => destinations,
        None => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer),
    };
    match node
        .node
        .broadcast(body, BroadcastOptions { destinations })
    {
        Ok(receipt) => {
            unsafe {
                *out_receipt = map_v1_receipt(receipt);
            }
            clear_v1_node_error(out_node_error)
        }
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_set_log_level(
    node: *mut RnsEmbeddedV1Node,
    level: RnsEmbeddedV1LogLevel,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    match node.node.set_log_level(map_v1_log_level(level)) {
        Ok(()) => clear_v1_node_error(out_node_error),
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_node_subscribe_events(
    node: *mut RnsEmbeddedV1Node,
    out_subscription: *mut *mut RnsEmbeddedEventSubscription,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    let Some(node) = v1_node_mut(node) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    if out_subscription.is_null() {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    }
    match node.node.subscribe_events() {
        Ok(subscription) => {
            unsafe {
                *out_subscription = Box::into_raw(Box::new(RnsEmbeddedEventSubscription { subscription }));
            }
            clear_v1_node_error(out_node_error)
        }
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_subscription_next(
    subscription: *mut RnsEmbeddedEventSubscription,
    timeout_ms: u64,
    out_poll_result: *mut RnsEmbeddedV1PollResult,
    out_event: *mut RnsEmbeddedV1NodeEvent,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    let Some(subscription) = v1_subscription_mut(subscription) else {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle);
    };
    if out_poll_result.is_null() || out_event.is_null() {
        return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidPointer);
    }
    match subscription.subscription.next(timeout_ms) {
        Ok(result) => {
            unsafe {
                *out_poll_result = map_v1_poll_result(&result);
                *out_event = match result {
                    PollResult::Event(event) => map_v1_event(&event),
                    _ => RnsEmbeddedV1NodeEvent::default(),
                };
            }
            clear_v1_node_error(out_node_error)
        }
        Err(err) => set_v1_node_error(out_node_error, err),
    }
}

#[no_mangle]
pub extern "C" fn rns_embedded_v1_subscription_close(
    subscription: *mut RnsEmbeddedEventSubscription,
    out_node_error: *mut RnsEmbeddedV1NodeError,
) -> RnsEmbeddedStatus {
    if subscription.is_null() {
        return clear_v1_node_error(out_node_error);
    }
    let boxed = unsafe { Box::from_raw(subscription) };
    match boxed.subscription.close() {
        Ok(()) => clear_v1_node_error(out_node_error),
        Err(err) => set_v1_node_error(out_node_error, err),
    }
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
    Some(unsafe { &mut *node })
}

fn v1_subscription_mut<'a>(
    subscription: *mut RnsEmbeddedEventSubscription,
) -> Option<&'a mut RnsEmbeddedEventSubscription> {
    if subscription.is_null() {
        return None;
    }
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
            capture_defaults: CaptureDefaults {
                max_bytes: config.capture_default_max_bytes,
            },
        },
        backend: NodeBackendConfig::Ble(BleNodeBackendConfig {
            mtu_hint: config.ble_mtu_hint,
            max_inbound_frames: config.ble_max_inbound_frames,
            max_outbound_frames: config.ble_max_outbound_frames,
            ordered_delivery: config.ble_ordered_delivery,
        }),
    })
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
        NodeEventKind::StatusChanged {
            run_state,
            lifecycle_state,
        } => {
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
        NodeEventKind::Error {
            error,
            frame_kind,
            sequence,
        } => {
            out.kind = RnsEmbeddedV1EventKind::Error;
            out.error_code = map_v1_node_error_code(error);
            out.frame_kind = *frame_kind;
            out.sequence = *sequence;
        }
        NodeEventKind::PacketReceived {
            frame_kind,
            sequence,
            bytes,
        } => {
            out.kind = RnsEmbeddedV1EventKind::PacketReceived;
            out.frame_kind = *frame_kind;
            out.sequence = *sequence;
            out.bytes = *bytes;
        }
        NodeEventKind::PacketSent {
            frame_kind,
            sequence,
            bytes,
        } => {
            out.kind = RnsEmbeddedV1EventKind::PacketSent;
            out.frame_kind = *frame_kind;
            out.sequence = *sequence;
            out.bytes = *bytes;
        }
        NodeEventKind::Extension {
            extension_id,
            value0,
            value1,
        } => {
            out.kind = RnsEmbeddedV1EventKind::Extension;
            out.extension_id = *extension_id;
            out.value0 = *value0;
            out.value1 = *value1;
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

fn clear_v1_node_error(out_node_error: *mut RnsEmbeddedV1NodeError) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
        unsafe {
            *out_node_error = RnsEmbeddedV1NodeError::default();
        }
    }
    RnsEmbeddedStatus::Ok
}

fn set_v1_pointer_error(
    out_node_error: *mut RnsEmbeddedV1NodeError,
    code: RnsEmbeddedV1NodeErrorCode,
) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
        unsafe {
            *out_node_error = RnsEmbeddedV1NodeError {
                code,
                ..RnsEmbeddedV1NodeError::default()
            };
        }
    }
    RnsEmbeddedStatus::InvalidArgument
}

fn set_v1_node_error(
    out_node_error: *mut RnsEmbeddedV1NodeError,
    error: NodeError,
) -> RnsEmbeddedStatus {
    if !out_node_error.is_null() {
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
    }
}

fn map_node_error_status(error: NodeError) -> RnsEmbeddedStatus {
    match error {
        NodeError::InvalidConfig => RnsEmbeddedStatus::InvalidInput,
        NodeError::IoError => RnsEmbeddedStatus::InvalidState,
        NodeError::NetworkError => RnsEmbeddedStatus::Disconnected,
        NodeError::ReticulumError => RnsEmbeddedStatus::InvalidState,
        NodeError::AlreadyRunning | NodeError::NotRunning | NodeError::InternalError => {
            RnsEmbeddedStatus::InvalidState
        }
        NodeError::Timeout => RnsEmbeddedStatus::Timeout,
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
        EmbeddedError::InvalidCursor | EmbeddedError::InvalidState => RnsEmbeddedStatus::InvalidState,
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
    use super::{
        RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT, RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST,
        RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI, RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING,
        RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
        RnsEmbeddedLinkState, RnsEmbeddedNodeConfig, RnsEmbeddedStatus, RnsEmbeddedV1Capabilities,
        RnsEmbeddedV1EventKind, RnsEmbeddedV1NodeEvent,
        RnsEmbeddedV1LogLevel, RnsEmbeddedV1NodeError, RnsEmbeddedV1NodeErrorCode,
        RnsEmbeddedV1NodeStatus, RnsEmbeddedV1PollResult, RnsEmbeddedV1PollResultKind,
        RnsEmbeddedV1RunState, RnsEmbeddedV1SendReceipt,
        rns_embedded_node_free, rns_embedded_node_new, rns_embedded_node_push_inbound_wire,
        rns_embedded_node_queue_message, rns_embedded_node_set_link_state,
        rns_embedded_node_take_outbound_wire, rns_embedded_node_tick, rns_embedded_v1_abi_version,
        rns_embedded_v1_get_capabilities, rns_embedded_v1_node_broadcast,
        rns_embedded_v1_node_config_default, rns_embedded_v1_node_free, rns_embedded_v1_node_get_status,
        rns_embedded_v1_node_new, rns_embedded_v1_node_restart, rns_embedded_v1_node_send,
        rns_embedded_v1_node_set_log_level, rns_embedded_v1_node_start, rns_embedded_v1_node_stop,
        rns_embedded_v1_node_subscribe_events, rns_embedded_v1_subscription_close,
        rns_embedded_v1_subscription_next,
    };
    use rns_embedded_core::packet::{PacketFrame, decode_frame, encode_frame};

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

        let mut capabilities = RnsEmbeddedV1Capabilities::default();
        assert_eq!(
            rns_embedded_v1_get_capabilities(&mut capabilities),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(capabilities.abi_version, 1);
        assert_ne!(capabilities.capability_bits, 0);
        assert_ne!(
            capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_BROADCAST_EXPLICIT_LIST,
            0
        );
        assert_ne!(
            capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_COMPAT_LEGACY_FFI,
            0
        );
        assert_ne!(
            capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_EVENT_GAP_SIGNALING,
            0
        );

        #[cfg(feature = "std")]
        {
            assert_ne!(
                capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
                0
            );
            assert_ne!(
                capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT,
                0
            );
        }

        #[cfg(not(feature = "std"))]
        {
            assert_eq!(
                capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_MANAGED_RUNTIME,
                0
            );
            assert_eq!(
                capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT,
                0
            );
        }

        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let mut config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        assert_eq!(
            rns_embedded_v1_node_start(node, &config, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(error.code, RnsEmbeddedV1NodeErrorCode::Unknown);

        let mut status = RnsEmbeddedV1NodeStatus::default();
        assert_eq!(
            rns_embedded_v1_node_get_status(node, &mut status),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(status.run_state, RnsEmbeddedV1RunState::Running);
        assert_eq!(status.epoch, 1);

        config.announce_interval_ms = 2_000;
        assert_eq!(
            rns_embedded_v1_node_restart(node, &config, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_node_get_status(node, &mut status),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(status.epoch, 2);

        assert_eq!(
            rns_embedded_v1_node_stop(node, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_node_get_status(node, &mut status),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(status.run_state, RnsEmbeddedV1RunState::Stopped);

        rns_embedded_v1_node_free(node);
    }

    #[test]
    fn ffi_v1_send_and_broadcast_surface_node_errors() {
        let node = rns_embedded_v1_node_new();
        assert!(!node.is_null());

        let config = rns_embedded_v1_node_config_default();
        let mut error = RnsEmbeddedV1NodeError::default();
        assert_eq!(
            rns_embedded_v1_node_start(node, &config, &mut error),
            RnsEmbeddedStatus::Ok
        );

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

        assert_eq!(
            rns_embedded_v1_node_start(node, &config, &mut error),
            RnsEmbeddedStatus::Ok
        );

        let mut poll = RnsEmbeddedV1PollResult::default();
        let mut event = RnsEmbeddedV1NodeEvent::default();
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::NodeRestarted);
        assert_eq!(poll.epoch, 1);

        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::Event);
        assert_eq!(event.kind, RnsEmbeddedV1EventKind::StatusChanged);
        assert_eq!(event.epoch, 1);

        assert_eq!(
            rns_embedded_v1_node_stop(node, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(
            rns_embedded_v1_subscription_next(subscription, 100, &mut poll, &mut event, &mut error),
            RnsEmbeddedStatus::Ok
        );
        assert_eq!(poll.kind, RnsEmbeddedV1PollResultKind::NodeStopped);

        assert_eq!(
            rns_embedded_v1_subscription_close(subscription, &mut error),
            RnsEmbeddedStatus::Ok
        );

        rns_embedded_v1_node_free(node);
    }
}
