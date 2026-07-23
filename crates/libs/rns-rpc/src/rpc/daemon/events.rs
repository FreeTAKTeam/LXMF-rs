use super::*;

impl RpcDaemon {
    pub(super) fn run_announce_scheduler(
        self: std::sync::Arc<Self>,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()>
    where
        Self: Sized,
    {
        let bridge = self.announce_bridge.clone();
        tokio::spawn(async move {
            if interval_secs == 0 {
                return;
            }

            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                let id = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);

                if let Some(bridge) = &bridge {
                    if let Err(error) = bridge.announce_now() {
                        log::warn!("scheduled announce failed: {error}");
                    }
                }

                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp, "announce_id": id }),
                };
                self.publish_event(event);
            }
        })
    }

    pub(super) fn try_until_capacity<F>(&self, block_timeout_ms: u64, mut attempt: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let timeout = block_timeout_ms.max(1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout);
        let mut spins = 0u32;
        loop {
            if attempt() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            if spins < 32 {
                std::hint::spin_loop();
                spins += 1;
            } else {
                std::thread::yield_now();
            }
        }
    }

    pub(super) fn sdk_overflow_policy(&self) -> String {
        let configured = self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("overflow_policy")
            .and_then(JsonValue::as_str)
            .unwrap_or("drop_oldest")
            .trim()
            .to_ascii_lowercase();
        if matches!(configured.as_str(), "reject" | "drop_oldest" | "block") {
            configured
        } else {
            "drop_oldest".to_string()
        }
    }

    pub(super) fn sdk_block_timeout_ms(&self) -> u64 {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("block_timeout_ms")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0)
    }

    pub(super) fn push_legacy_event_with_policy(
        &self,
        event: &RpcEvent,
        policy: &str,
        block_timeout_ms: u64,
    ) -> bool {
        match policy {
            "reject" => {
                let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
                if guard.len() >= LEGACY_EVENT_QUEUE_CAPACITY {
                    return false;
                }
                guard.push_back(event.clone());
                true
            }
            "block" => self.try_until_capacity(block_timeout_ms, || {
                let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
                if guard.len() < LEGACY_EVENT_QUEUE_CAPACITY {
                    guard.push_back(event.clone());
                    true
                } else {
                    false
                }
            }),
            _ => {
                let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
                if guard.len() >= LEGACY_EVENT_QUEUE_CAPACITY {
                    guard.pop_front();
                }
                guard.push_back(event.clone());
                true
            }
        }
    }

    pub(super) fn push_sdk_event_log_with_policy(
        &self,
        sequenced_event: SequencedRpcEvent,
        policy: &str,
        block_timeout_ms: u64,
    ) -> bool {
        match policy {
            "reject" => {
                let mut log_guard =
                    self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
                if log_guard.len() >= SDK_EVENT_LOG_CAPACITY {
                    return false;
                }
                log_guard.push_back(sequenced_event);
                true
            }
            "block" => self.try_until_capacity(block_timeout_ms, move || {
                let mut log_guard =
                    self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
                if log_guard.len() < SDK_EVENT_LOG_CAPACITY {
                    log_guard.push_back(sequenced_event.clone());
                    true
                } else {
                    false
                }
            }),
            _ => {
                let mut log_guard =
                    self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
                if log_guard.len() >= SDK_EVENT_LOG_CAPACITY {
                    log_guard.pop_front();
                    let mut dropped = self
                        .sdk_dropped_event_count
                        .lock()
                        .expect("sdk_dropped_event_count mutex poisoned");
                    *dropped = dropped.saturating_add(1);
                    self.metrics_record_event_drop();
                }
                log_guard.push_back(sequenced_event);
                true
            }
        }
    }

    pub fn handle_framed_request(&self, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        self.handle_framed_request_for_session(super::LEGACY_RPC_SESSION_ID, bytes)
    }

    pub fn handle_framed_request_for_session(
        &self,
        session_id: &str,
        bytes: &[u8],
    ) -> Result<Vec<u8>, std::io::Error> {
        let request: RpcRequest = codec::decode_frame(bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let response = super::with_rpc_session(session_id, || self.handle_rpc(request))?;
        codec::encode_frame(&response).map_err(std::io::Error::other)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<RpcEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_sdk_events(&self) -> broadcast::Receiver<SequencedRpcEvent> {
        self.sdk_events.subscribe()
    }

    pub fn take_event(&self) -> Option<RpcEvent> {
        let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
        guard.pop_front()
    }

    fn push_sequenced_event(&self, event: RpcEvent) -> SequencedRpcEvent {
        let event = self.redact_event(event);
        let policy = self.sdk_overflow_policy();
        let block_timeout_ms = self.sdk_block_timeout_ms();

        if !self.push_legacy_event_with_policy(&event, policy.as_str(), block_timeout_ms) {
            log::warn!(
                "legacy event queue rejected event type={} overflow_policy={policy}",
                event.event_type
            );
        }

        let seq_no = {
            let mut seq_guard =
                self.sdk_next_event_seq.lock().expect("sdk_next_event_seq mutex poisoned");
            *seq_guard = seq_guard.saturating_add(1);
            *seq_guard
        };
        let sequenced_event = SequencedRpcEvent { seq_no, event: event.clone() };
        let inserted = self.push_sdk_event_log_with_policy(
            sequenced_event.clone(),
            policy.as_str(),
            block_timeout_ms,
        );
        if !inserted {
            let mut dropped = self
                .sdk_dropped_event_count
                .lock()
                .expect("sdk_dropped_event_count mutex poisoned");
            *dropped = dropped.saturating_add(1);
            self.metrics_record_event_drop();
        }
        self.dispatch_event_sink_bridges(seq_no, &event);
        sequenced_event
    }

    pub fn push_event(&self, event: RpcEvent) -> RpcEvent {
        self.push_sequenced_event(event).event
    }

    pub fn publish_event(&self, event: RpcEvent) {
        let sequenced_event = self.push_sequenced_event(event);
        let seq_no = sequenced_event.seq_no;
        let event_type = sequenced_event.event.event_type.clone();
        if self.events.send(sequenced_event.event.clone()).is_err() {
            log::trace!(
                "[rpc-daemon] legacy event has no active subscribers seq_no={seq_no} event_type={event_type}"
            );
        }
        if self.sdk_events.send(sequenced_event).is_err() {
            log::trace!(
                "[rpc-daemon] sdk event has no active subscribers seq_no={seq_no} event_type={event_type}"
            );
        }
    }

    pub fn sdk_stream_event_frame(&self, sequenced_event: &SequencedRpcEvent) -> JsonValue {
        json!({
            "event_id": format!("evt-{}", sequenced_event.seq_no),
            "runtime_id": self.identity_hash,
            "stream_id": SDK_STREAM_ID,
            "seq_no": sequenced_event.seq_no,
            "contract_version": self.active_contract_version(),
            "ts_ms": (now_i64().max(0) as u64) * 1000,
            "event_type": sequenced_event.event.event_type.clone(),
            "severity": Self::event_severity(sequenced_event.event.event_type.as_str()),
            "source_component": "rns-rpc",
            "payload": sequenced_event.event.payload.clone(),
        })
    }

    pub fn sdk_stream_gap_frame(
        &self,
        expected_seq_no: u64,
        observed_seq_no: u64,
        dropped_count: u64,
    ) -> JsonValue {
        let gap_seq_no = observed_seq_no.saturating_sub(1);
        json!({
            "event_id": format!("gap-{}", gap_seq_no),
            "runtime_id": self.identity_hash,
            "stream_id": SDK_STREAM_ID,
            "seq_no": gap_seq_no,
            "contract_version": self.active_contract_version(),
            "ts_ms": (now_i64().max(0) as u64) * 1000,
            "event_type": "StreamGap",
            "severity": "warn",
            "source_component": "rns-rpc",
            "payload": {
                "expected_seq_no": expected_seq_no,
                "observed_seq_no": observed_seq_no,
                "dropped_count": dropped_count,
                "recovery_required": true,
            },
        })
    }

    pub fn emit_event(&self, event: RpcEvent) {
        self.publish_event(event);
    }

    pub fn schedule_announce_for_test(&self, id: u64) {
        let timestamp = now_i64();
        let event = RpcEvent {
            event_type: "announce_sent".into(),
            payload: json!({ "timestamp": timestamp, "announce_id": id }),
        };
        self.publish_event(event);
    }

    pub fn start_announce_scheduler(
        self: std::rc::Rc<Self>,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_local(async move {
            if interval_secs == 0 {
                return;
            }

            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                let id = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);

                if let Some(bridge) = &self.announce_bridge {
                    if let Err(error) = bridge.announce_now() {
                        log::warn!("scheduled announce failed: {error}");
                    }
                }

                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp, "announce_id": id }),
                };
                self.publish_event(event);
            }
        })
    }

    pub fn start_announce_scheduler_shared(
        self: std::sync::Arc<Self>,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        self.run_announce_scheduler(interval_secs)
    }

    pub fn inject_inbound_test_message(&self, content: &str) {
        let timestamp = now_i64();
        let record = crate::storage::messages::MessageRecord {
            id: format!("test-{}", timestamp),
            source: "test-peer".into(),
            destination: "local".into(),
            title: "".into(),
            content: content.into(),
            timestamp,
            direction: "in".into(),
            fields: None,
            receipt_status: None,
        };
        if let Err(error) = self.store.insert_message(&record) {
            log::error!(
                "failed to persist injected inbound test message id={}: {error}",
                record.id
            );
            return;
        }
        let event =
            RpcEvent { event_type: "inbound".into(), payload: json!({ "message": record }) };
        self.publish_event(event);
    }

    pub fn emit_link_event_for_test(&self) {
        let event = RpcEvent {
            event_type: "link_activated".into(),
            payload: json!({ "link_id": "test-link" }),
        };
        self.publish_event(event);
    }
}
