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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return RnsEmbeddedStatus::InvalidArgument,
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
            Ok(destinations) => destinations,
            Err(_) => {
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
        let node = match v1_node_mut(node) {
            Ok(n) => n,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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
        let subscription = match v1_subscription_mut(subscription) {
            Ok(s) => s,
            Err(_) => return set_v1_pointer_error(out_node_error, RnsEmbeddedV1NodeErrorCode::InvalidHandle),
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

fn v1_node_mut<'a>(node: *mut RnsEmbeddedV1Node) -> Result<&'a mut RnsEmbeddedV1Node, &'static str> {
    if node.is_null() {
        return Err("null node pointer");
    }
    // SAFETY: callers pass handles allocated by this crate and this helper only
    // creates a temporary exclusive borrow for the duration of the FFI call.
    Ok(unsafe { &mut *node })
}

fn v1_subscription_mut<'a>(
    subscription: *mut RnsEmbeddedEventSubscription,
) -> Result<&'a mut RnsEmbeddedEventSubscription, &'static str> {
    if subscription.is_null() {
        return Err("null subscription pointer");
    }
    // SAFETY: callers pass handles allocated by this crate and this helper only
    // creates a temporary exclusive borrow for the duration of the FFI call.
    Ok(unsafe { &mut *subscription })
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
