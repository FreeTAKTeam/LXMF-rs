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
