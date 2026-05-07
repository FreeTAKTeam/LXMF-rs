use super::*;

impl RpcDaemon {
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
            match sink.publish(&envelope) {
                Ok(()) => self.metrics_record_event_sink_publish(sink_kind.as_str()),
                Err(_) => self.metrics_record_event_sink_error(sink_kind.as_str()),
            }
        }
    }
}
