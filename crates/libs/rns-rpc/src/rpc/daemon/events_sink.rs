use super::*;

const EVENT_SINK_QUEUE_CAPACITY: usize = 1024;

impl RpcDaemon {
    pub(super) fn spawn_event_sink_worker(
        metrics: Arc<Mutex<RpcMetrics>>,
    ) -> std::io::Result<mpsc::SyncSender<EventSinkCommand>> {
        let (tx, rx) = mpsc::sync_channel::<EventSinkCommand>(EVENT_SINK_QUEUE_CAPACITY);
        std::thread::Builder::new().name("rpc-event-sink-worker".to_string()).spawn(move || {
            while let Ok(command) = rx.recv() {
                match command {
                    EventSinkCommand::Publish { sink, sink_kind, envelope } => {
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
                        reply.send(()).expect("event sink flush requester remains connected");
                    }
                }
            }
        })?;
        Ok(tx)
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

    pub(super) fn sdk_event_sink_allowed_kinds(&self) -> std::io::Result<Option<HashSet<String>>> {
        let config =
            self.sdk_runtime_config.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        let Some(kinds) = config
            .get("event_sink")
            .and_then(|value| value.get("allow_kinds"))
            .and_then(JsonValue::as_array)
        else {
            return Ok(None);
        };
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
        Ok((!allowed.is_empty()).then_some(allowed))
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
            contract_release: "v2.6".to_string(),
            runtime_id: self.identity_hash.clone(),
            stream_id: SDK_STREAM_ID.to_string(),
            seq_no,
            emitted_at_ms: now_i64(),
            event: event.clone(),
        };
        let max_event_bytes = self.sdk_event_sink_max_event_bytes();
        let event_bytes = match serde_json::to_vec(&envelope) {
            Ok(payload) => payload.len(),
            Err(error) => {
                log::error!(
                    "[daemon] failed to encode event sink envelope type={}: {error}",
                    event.event_type
                );
                self.metrics_record_event_sink_skipped();
                return;
            }
        };
        if event_bytes > max_event_bytes {
            self.metrics_record_event_sink_skipped();
            return;
        }
        let allowed_kinds = self.sdk_event_sink_allowed_kinds().unwrap_or_else(|err| {
            log::warn!("[daemon] event sink allowed_kinds lock error: {err}");
            None
        });

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
