impl RpcDaemon {
    fn sdk_overflow_policy(&self) -> String {
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

    fn sdk_block_timeout_ms(&self) -> u64 {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("block_timeout_ms")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0)
    }

    fn push_legacy_event_with_policy(
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
            "block" => {
                let timeout = block_timeout_ms.max(1);
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(timeout);
                loop {
                    {
                        let mut guard =
                            self.event_queue.lock().expect("event_queue mutex poisoned");
                        if guard.len() < LEGACY_EVENT_QUEUE_CAPACITY {
                            guard.push_back(event.clone());
                            return true;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
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

    fn push_sdk_event_log_with_policy(
        &self,
        sequenced_event: SequencedRpcEvent,
        policy: &str,
        block_timeout_ms: u64,
    ) -> bool {
        match policy {
            "reject" => {
                let mut log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
                if log_guard.len() >= SDK_EVENT_LOG_CAPACITY {
                    return false;
                }
                log_guard.push_back(sequenced_event);
                true
            }
            "block" => {
                let timeout = block_timeout_ms.max(1);
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(timeout);
                loop {
                    {
                        let mut log_guard =
                            self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
                        if log_guard.len() < SDK_EVENT_LOG_CAPACITY {
                            log_guard.push_back(sequenced_event);
                            return true;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            _ => {
                let mut log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
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

    fn redaction_enabled(&self) -> bool {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("redaction")
            .and_then(|value| value.get("enabled"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(true)
    }

    fn redaction_transform(&self) -> &'static str {
        match self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("redaction")
            .and_then(|value| value.get("sensitive_transform"))
            .and_then(JsonValue::as_str)
            .unwrap_or("hash")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "truncate" => "truncate",
            "redact" => "redact",
            _ => "hash",
        }
    }

    fn sdk_event_sink_enabled(&self) -> bool {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("event_sink")
            .and_then(|value| value.get("enabled"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    }

    fn sdk_event_sink_max_event_bytes(&self) -> usize {
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

    fn sdk_event_sink_allowed_kinds(&self) -> Option<HashSet<String>> {
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

    fn dispatch_event_sink_bridges(&self, seq_no: u64, event: &RpcEvent) {
        if self.event_sink_bridges.is_empty() || !self.sdk_event_sink_enabled() {
            return;
        }

        let envelope = RpcEventSinkEnvelope {
            contract_release: "v2.5".to_string(),
            runtime_id: self.identity_hash.clone(),
            stream_id: SDK_STREAM_ID.to_string(),
            seq_no,
            emitted_at_ms: now_i64(),
            event: event.clone(),
        };
        let max_event_bytes = self.sdk_event_sink_max_event_bytes();
        let event_bytes = serde_json::to_vec(&envelope).map(|payload| payload.len()).unwrap_or(usize::MAX);
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
            match sink.publish(&envelope) {
                Ok(()) => self.metrics_record_event_sink_publish(sink_kind.as_str()),
                Err(_) => self.metrics_record_event_sink_error(sink_kind.as_str()),
            }
        }
    }

    fn is_sensitive_key(key: &str) -> bool {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "peer_id"
                | "destination_hash"
                | "correlation_id"
                | "trace_id"
                | "source_ip"
                | "principal"
                | "shared_secret"
                | "authorization"
                | "token"
                | "passphrase"
        )
    }

    fn redact_scalar(value: &str, transform: &str) -> String {
        match transform {
            "truncate" => {
                let preview = value.chars().take(8).collect::<String>();
                if value.chars().count() <= 8 {
                    preview
                } else {
                    format!("{preview}...")
                }
            }
            "redact" => "[redacted]".to_string(),
            _ => {
                let mut hasher = Sha256::new();
                hasher.update(value.as_bytes());
                let digest = hex::encode(hasher.finalize());
                format!("sha256:{}", &digest[..16])
            }
        }
    }

    fn redact_sensitive_value(value: &mut JsonValue, transform: &str) {
        let replacement = match value {
            JsonValue::String(current) => Self::redact_scalar(current, transform),
            _ => Self::redact_scalar(value.to_string().as_str(), transform),
        };
        *value = JsonValue::String(replacement);
    }

    fn redact_json_value(value: &mut JsonValue, transform: &str) {
        match value {
            JsonValue::Object(map) => {
                for (key, inner) in map.iter_mut() {
                    if Self::is_sensitive_key(key) {
                        Self::redact_sensitive_value(inner, transform);
                    } else {
                        Self::redact_json_value(inner, transform);
                    }
                }
            }
            JsonValue::Array(items) => {
                for item in items.iter_mut() {
                    Self::redact_json_value(item, transform);
                }
            }
            _ => {}
        }
    }

    fn redact_event(&self, mut event: RpcEvent) -> RpcEvent {
        if !self.redaction_enabled() {
            return event;
        }
        let transform = self.redaction_transform();
        Self::redact_json_value(&mut event.payload, transform);
        event
    }

    pub fn handle_framed_request(&self, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let request: RpcRequest = codec::decode_frame(bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let response = self.handle_rpc(request)?;
        codec::encode_frame(&response).map_err(std::io::Error::other)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<RpcEvent> {
        self.events.subscribe()
    }

    pub fn take_event(&self) -> Option<RpcEvent> {
        let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
        guard.pop_front()
    }

    pub fn push_event(&self, event: RpcEvent) -> RpcEvent {
        let event = self.redact_event(event);
        let policy = self.sdk_overflow_policy();
        let block_timeout_ms = self.sdk_block_timeout_ms();

        let _ = self.push_legacy_event_with_policy(&event, policy.as_str(), block_timeout_ms);

        let seq_no = {
            let mut seq_guard =
                self.sdk_next_event_seq.lock().expect("sdk_next_event_seq mutex poisoned");
            *seq_guard = seq_guard.saturating_add(1);
            *seq_guard
        };
        let inserted = self.push_sdk_event_log_with_policy(
            SequencedRpcEvent { seq_no, event: event.clone() },
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
        event
    }

    pub fn publish_event(&self, event: RpcEvent) {
        let event = self.push_event(event);
        let _ = self.events.send(event);
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
                // First tick is immediate, so we announce once at scheduler start.
                interval.tick().await;
                let id = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);

                if let Some(bridge) = &self.announce_bridge {
                    let _ = bridge.announce_now();
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
        let _ = self.store.insert_message(&record);
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
