pub struct EventSubscription {
    inner: SharedNode,
    subscription_id: u64,
}

impl Clone for EventSubscription {
    fn clone(&self) -> Self {
        Self { inner: clone_inner(&self.inner), subscription_id: self.subscription_id }
    }
}

impl EmbeddedNode {
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        let inner = Arc::new(StdNodeInner {
            state: Mutex::new(NodeState::default()),
            condvar: Condvar::new(),
        });

        #[cfg(not(feature = "std"))]
        let inner = Rc::new(RefCell::new(NodeState::default()));

        Self { inner }
    }

    pub fn start(&self, config: NodeConfig) -> Result<(), NodeError> {
        let next_epoch = self.with_state(|state| {
            if state.session.is_some() {
                return Err(NodeError::AlreadyRunning);
            }
            let epoch = state.epoch.saturating_add(1);
            let session = RuntimeSession::new(epoch, &config)?;
            state.epoch = epoch;
            state.session = Some(session);
            state.event_capacity = config.runtime.max_events;
            state.last_now_ms = 0;
            signal_generation_change(state, epoch);
            push_event_locked(
                state,
                NodeEventKind::StatusChanged {
                    run_state: NodeRunState::Running,
                    lifecycle_state: Some(NodeLifecycleState::Boot),
                },
                None,
                0,
            );
            Ok(epoch)
        })?;
        self.notify_waiters();
        #[cfg(feature = "std")]
        self.start_driver(next_epoch);
        #[cfg(not(feature = "std"))]
        let _ = next_epoch;
        Ok(())
    }

    pub fn stop(&self) -> Result<(), NodeError> {
        #[cfg(feature = "std")]
        let handle = self.with_state(|state| {
            let handle = stop_driver_locked(state);
            if state.session.is_some() {
                state.session = None;
                signal_stopped(state);
                push_event_locked(
                    state,
                    NodeEventKind::StatusChanged {
                        run_state: NodeRunState::Stopped,
                        lifecycle_state: None,
                    },
                    None,
                    state.last_now_ms,
                );
            }
            Ok(handle)
        })?;

        #[cfg(not(feature = "std"))]
        self.with_state(|state| {
            if state.session.is_some() {
                state.session = None;
                signal_stopped(state);
                push_event_locked(
                    state,
                    NodeEventKind::StatusChanged {
                        run_state: NodeRunState::Stopped,
                        lifecycle_state: None,
                    },
                    None,
                    state.last_now_ms,
                );
            }
            Ok(())
        })?;

        self.notify_waiters();

        #[cfg(feature = "std")]
        join_driver(handle);

        Ok(())
    }

    pub fn restart(&self, config: NodeConfig) -> Result<(), NodeError> {
        self.stop()?;
        self.start(config)
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
        let receipt = self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            if !session.has_outbound_capacity(1) {
                return Err(NodeError::QueuePressure);
            }
            let sequence = session.queue_message(destination, data)?;
            Ok(NodeOperationReceipt {
                operation: NodeOperationKind::Send,
                operation_id: u64::from(sequence),
                epoch: session.epoch,
                accepted_bytes: data.len(),
                queued: true,
                target_count: 1,
            })
        })?;
        self.notify_waiters();
        Ok(receipt)
    }

    pub fn broadcast(
        &self,
        data: &[u8],
        options: BroadcastOptions,
    ) -> Result<NodeOperationReceipt, NodeError> {
        if options.destinations.is_empty() {
            return Err(NodeError::InvalidConfig);
        }
        let receipt = self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            if !session.has_outbound_capacity(options.destinations.len()) {
                return Err(NodeError::QueuePressure);
            }
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
        })?;
        self.notify_waiters();
        Ok(receipt)
    }

    pub fn set_log_level(&self, level: NodeLogLevel) -> Result<(), NodeError> {
        self.with_state(|state| {
            state.log_level = level;
            push_event_locked(
                state,
                NodeEventKind::Log { level, code: 0 },
                None,
                state.last_now_ms,
            );
            Ok(())
        })?;
        self.notify_waiters();
        Ok(())
    }

    pub fn subscribe_events(&self) -> Result<EventSubscription, NodeError> {
        let subscription_id = self.with_state(|state| {
            let id = state.next_subscription_id;
            state.next_subscription_id = state.next_subscription_id.saturating_add(1);
            state.subscriptions.insert(
                id,
                SubscriptionState {
                    next_event_id: state.next_event_id,
                    pending_signals: VecDeque::new(),
                },
            );
            Ok(id)
        })?;

        Ok(EventSubscription { inner: clone_inner(&self.inner), subscription_id })
    }

    pub fn tick(&self, now_ms: u64) -> Result<(), NodeError> {
        self.with_state(|state| {
            ensure_manual_progression_allowed(state)?;
            let events = {
                let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
                session.tick(now_ms)?
            };
            state.last_now_ms = now_ms;
            append_runtime_events_locked(state, events);
            Ok(())
        })?;
        self.notify_waiters();
        Ok(())
    }

    pub fn set_link_state(&self, state_value: LinkState) -> Result<(), NodeError> {
        self.with_state(|state| {
            let session = state.session.as_mut().ok_or(NodeError::NotRunning)?;
            session.backend.set_link_state(state_value);
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

    pub fn capability_supports_blocking_next(&self) -> bool {
        cfg!(feature = "std")
    }

    pub fn capability_supports_managed_runtime(&self) -> bool {
        cfg!(feature = "std")
    }

    pub fn capability_supports_event_gap_signaling(&self) -> bool {
        true
    }

    #[cfg(feature = "std")]
    fn start_driver(&self, epoch: u64) {
        let start_instant = Instant::now();
        if let Ok(mut state) = self.inner.state.lock() {
            state.driver =
                Some(DriverState { epoch, stop_requested: false, start_instant, handle: None });
        }

        let inner = Arc::clone(&self.inner);
        let handle = thread::spawn(move || loop {
            let continue_running = driver_tick(&inner, epoch);
            if !continue_running {
                break;
            }
            thread::sleep(DRIVER_TICK_SLEEP);
        });

        if let Ok(mut state) = self.inner.state.lock() {
            if let Some(driver) = state.driver.as_mut() {
                if driver.epoch == epoch {
                    driver.handle = Some(handle);
                    return;
                }
            }
        }

        let _ = handle.join();
    }

    #[cfg(feature = "std")]
    fn notify_waiters(&self) {
        self.inner.condvar.notify_all();
    }

    #[cfg(not(feature = "std"))]
    fn notify_waiters(&self) {}

    #[cfg(feature = "std")]
    fn with_state<R>(
        &self,
        f: impl FnOnce(&mut NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let mut state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
        f(&mut state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state<R>(
        &self,
        f: impl FnOnce(&mut NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let mut state = self.inner.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
        f(&mut state)
    }

    #[cfg(feature = "std")]
    fn with_state_read<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        let state = self.inner.state.lock().expect("node state poisoned");
        f(&state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state_read<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        let state = self.inner.borrow();
        f(&state)
    }

    #[cfg(feature = "std")]
    fn with_state_read_result<R>(
        &self,
        f: impl FnOnce(&NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
        f(&state)
    }

    #[cfg(not(feature = "std"))]
    fn with_state_read_result<R>(
        &self,
        f: impl FnOnce(&NodeState) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let state = self.inner.try_borrow().map_err(|_| NodeError::InternalError)?;
        f(&state)
    }
}

impl EventSubscription {
    pub fn next(&self, timeout_ms: u64) -> Result<PollResult, NodeError> {
        #[cfg(feature = "std")]
        {
            if timeout_ms > MAX_BLOCKING_TIMEOUT_MS {
                return Ok(PollResult::Timeout);
            }

            let Some(deadline) = Instant::now().checked_add(Duration::from_millis(timeout_ms))
            else {
                return Ok(PollResult::Timeout);
            };
            let mut state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
            loop {
                let result = next_poll_result_locked(&mut state, self.subscription_id);
                if !matches!(result, PollResult::Timeout) || timeout_ms == 0 {
                    return Ok(result);
                }
                let now = Instant::now();
                if now >= deadline {
                    return Ok(PollResult::Timeout);
                }
                let wait = deadline.saturating_duration_since(now);
                let (next_state, _) = self
                    .inner
                    .condvar
                    .wait_timeout(state, wait)
                    .map_err(|_| NodeError::InternalError)?;
                state = next_state;
            }
        }

        #[cfg(not(feature = "std"))]
        {
            let mut state = self.inner.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
            let _ = timeout_ms;
            Ok(next_poll_result_locked(&mut state, self.subscription_id))
        }
    }

    pub fn close(&self) -> Result<(), NodeError> {
        #[cfg(feature = "std")]
        {
            let mut state = self.inner.state.lock().map_err(|_| NodeError::InternalError)?;
            state.subscriptions.remove(&self.subscription_id);
            self.inner.condvar.notify_all();
            Ok(())
        }

        #[cfg(not(feature = "std"))]
        {
            let mut state = self.inner.try_borrow_mut().map_err(|_| NodeError::InternalError)?;
            state.subscriptions.remove(&self.subscription_id);
            Ok(())
        }
    }
}
