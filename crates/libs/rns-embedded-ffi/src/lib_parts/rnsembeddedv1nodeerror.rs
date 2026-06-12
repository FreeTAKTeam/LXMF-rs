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
