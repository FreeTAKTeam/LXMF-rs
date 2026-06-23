fn destination_list(
    ptr: *const u8,
    count: usize,
) -> Result<alloc::vec::Vec<[u8; 16]>, &'static str> {
    if count == 0 {
        return Ok(alloc::vec::Vec::new());
    }
    if ptr.is_null() {
        return Err("null destination pointer with non-zero count");
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
    Ok(out)
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
