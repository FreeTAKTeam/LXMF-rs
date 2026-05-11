use super::*;

const EVENT_SINK_QUEUE_CAPACITY: usize = 1024;
const EVENT_SINK_WORKERS: usize = 4;

struct EventSinkWorkerState {
    in_flight: Mutex<usize>,
    idle: Condvar,
}

impl EventSinkWorkerState {
    fn new() -> Self {
        Self { in_flight: Mutex::new(0), idle: Condvar::new() }
    }

    fn publish_started(self: &Arc<Self>) -> EventSinkInFlight {
        let mut in_flight = self.in_flight.lock().expect("event sink state mutex poisoned");
        *in_flight = in_flight.saturating_add(1);
        EventSinkInFlight { state: Arc::clone(self) }
    }

    #[cfg(test)]
    fn wait_idle(&self) {
        let mut in_flight = self.in_flight.lock().expect("event sink state mutex poisoned");
        while *in_flight != 0 {
            in_flight = self.idle.wait(in_flight).expect("event sink state mutex poisoned");
        }
    }
}

struct EventSinkInFlight {
    state: Arc<EventSinkWorkerState>,
}

impl Drop for EventSinkInFlight {
    fn drop(&mut self) {
        let mut in_flight = self.state.in_flight.lock().expect("event sink state mutex poisoned");
        *in_flight = in_flight.saturating_sub(1);
        if *in_flight == 0 {
            self.state.idle.notify_all();
        }
    }
}

impl RpcDaemon {
    pub(super) fn spawn_event_sink_worker(
        enabled: bool,
        metrics: Arc<Mutex<RpcMetrics>>,
    ) -> Option<mpsc::SyncSender<EventSinkCommand>> {
        if !enabled {
            return None;
        }
        let (tx, rx) = mpsc::sync_channel::<EventSinkCommand>(EVENT_SINK_QUEUE_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        let worker_state = Arc::new(EventSinkWorkerState::new());
        for worker_index in 0..EVENT_SINK_WORKERS {
            let rx = Arc::clone(&rx);
            let metrics = Arc::clone(&metrics);
            let worker_state = Arc::clone(&worker_state);
            std::thread::Builder::new()
                .name(format!("rpc-event-sink-worker-{worker_index}"))
                .spawn(move || loop {
                    let command = {
                        let rx = rx.lock().expect("event sink receiver mutex poisoned");
                        rx.recv()
                    };
                    let Ok(command) = command else {
                        break;
                    };
                    match command {
                        EventSinkCommand::Publish { sink, sink_kind, envelope } => {
                            let _in_flight = worker_state.publish_started();
                            let result = sink.publish(&envelope);
                            let mut metrics = metrics.lock().expect("sdk_metrics mutex poisoned");
                            match result {
                                Ok(()) => {
                                    metrics.sdk_event_sink_publish_total =
                                        metrics.sdk_event_sink_publish_total.saturating_add(1);
                                    Self::metrics_increment(
                                        &mut metrics.sdk_event_sink_publish_by_kind,
                                        sink_kind.as_str(),
                                    );
                                }
                                Err(_) => {
                                    metrics.sdk_event_sink_error_total =
                                        metrics.sdk_event_sink_error_total.saturating_add(1);
                                    Self::metrics_increment(
                                        &mut metrics.sdk_event_sink_errors_by_kind,
                                        sink_kind.as_str(),
                                    );
                                }
                            }
                        }
                        #[cfg(test)]
                        EventSinkCommand::Flush { reply } => {
                            worker_state.wait_idle();
                            let _ = reply.send(());
                        }
                    }
                })
                .expect("spawn rpc event sink worker");
        }
        Some(tx)
    }

    pub(super) fn sdk_event_sink_enabled(&self) -> bool {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("event_sink")
            .and_then(|value| value.get("enabled"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    }

    pub(super) fn sdk_event_sink_max_event_bytes(&self) -> usize {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("event_sink")
            .and_then(|value| value.get("max_event_bytes"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value >= 256)
            .unwrap_or(65_536)
    }

    pub(super) fn sdk_event_sink_allowed_kinds(&self) -> Option<HashSet<String>> {
        let config = self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
        let kinds = config
            .get("event_sink")
            .and_then(|value| value.get("allow_kinds"))
            .and_then(JsonValue::as_array)?;
        let mut allowed = HashSet::new();
        for kind in kinds {
            if let Some(normalized) = kind
                .as_str()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .filter(|value| !value.is_empty())
            {
                allowed.insert(normalized);
            }
        }
        if allowed.is_empty() {
            None
        } else {
            Some(allowed)
        }
    }

    pub(super) fn dispatch_event_sink_bridges(&self, seq_no: u64, event: &RpcEvent) {
        if self.event_sink_bridges.is_empty() || !self.sdk_event_sink_enabled() {
            return;
        }
        let Some(event_sink_tx) = &self.event_sink_tx else {
            self.metrics_record_event_sink_skipped();
            return;
        };

        let envelope = RpcEventSinkEnvelope {
            contract_release: "v2.5".to_string(),
            runtime_id: self.identity_hash.clone(),
            stream_id: SDK_STREAM_ID.to_string(),
            seq_no,
            emitted_at_ms: now_i64(),
            event: event.clone(),
        };
        let max_event_bytes = self.sdk_event_sink_max_event_bytes();
        let event_bytes =
            serde_json::to_vec(&envelope).map(|payload| payload.len()).unwrap_or(usize::MAX);
        if event_bytes > max_event_bytes {
            self.metrics_record_event_sink_skipped();
            return;
        }
        let allowed_kinds = self.sdk_event_sink_allowed_kinds();

        for sink in &self.event_sink_bridges {
            let sink_kind = sink.sink_kind().trim().to_ascii_lowercase();
            if let Some(allowed) = allowed_kinds.as_ref() {
                if !allowed.contains(&sink_kind) {
                    self.metrics_record_event_sink_skipped();
                    continue;
                }
            }
            let command = EventSinkCommand::Publish {
                sink: sink.clone(),
                sink_kind,
                envelope: envelope.clone(),
            };
            match event_sink_tx.try_send(command) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) | Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.metrics_record_event_sink_skipped();
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn flush_event_sink_worker_for_test(&self) {
        let Some(event_sink_tx) = &self.event_sink_tx else {
            return;
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        if event_sink_tx.send(EventSinkCommand::Flush { reply: reply_tx }).is_ok() {
            let _ = reply_rx.recv_timeout(std::time::Duration::from_secs(1));
        }
    }
}
