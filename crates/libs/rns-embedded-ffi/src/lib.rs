#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

use alloc::boxed::Box;
use rns_embedded_core::{EmbeddedError, store::JournaledEmbeddedStore, transport::LinkState};
use rns_embedded_runtime::{
    EmbeddedNodeRuntime, RuntimeConfig,
    ble::{BleShimConfig, BleShimTransport},
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

pub struct RnsEmbeddedNode {
    runtime: EmbeddedNodeRuntime,
    store: JournaledEmbeddedStore,
    transport: BleShimTransport,
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

fn node_mut<'a>(node: *mut RnsEmbeddedNode) -> Option<&'a mut RnsEmbeddedNode> {
    if node.is_null() {
        return None;
    }
    // SAFETY: caller passes a pointer returned by `rns_embedded_node_new`; this
    // helper only creates a temporary exclusive borrow for the duration of the call.
    Some(unsafe { &mut *node })
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
        RnsEmbeddedLinkState, RnsEmbeddedNodeConfig, RnsEmbeddedStatus, rns_embedded_node_free,
        rns_embedded_node_new, rns_embedded_node_push_inbound_wire, rns_embedded_node_queue_message,
        rns_embedded_node_set_link_state, rns_embedded_node_take_outbound_wire,
        rns_embedded_node_tick,
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
}
