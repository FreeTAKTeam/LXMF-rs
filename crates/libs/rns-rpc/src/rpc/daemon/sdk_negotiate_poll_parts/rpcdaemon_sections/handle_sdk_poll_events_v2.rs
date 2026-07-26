impl RpcDaemon {

    #[allow(clippy::result_large_err)]
    pub(super) fn handle_sdk_poll_events_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPollEventsV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let clear_degraded_on_success = {
            let degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            if *degraded && parsed.cursor.is_some() {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_STREAM_DEGRADED",
                    "stream is degraded; reset cursor to recover",
                ));
            }
            *degraded && parsed.cursor.is_none()
        };

        if parsed.max == 0 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "poll max must be greater than zero",
            ));
        }

        let max_poll_events = self.sdk_max_poll_events();
        if parsed.max > max_poll_events {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED",
                "poll max exceeds supported limit",
            ));
        }
        let max_event_bytes = self.sdk_max_event_bytes();
        let max_batch_bytes = self.sdk_max_batch_bytes();
        let max_extension_keys = self.sdk_max_extension_keys();

        let cursor_seq = match self.sdk_decode_cursor(parsed.cursor.as_deref()) {
            Ok(value) => value,
            Err(error) => {
                return Ok(self.sdk_error_response(request.id, &error.code, &error.message))
            }
        };

        let log_lock_started = std::time::Instant::now();
        let log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
        let log_lock_wait_ns =
            log_lock_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.metrics_record_sdk_poll_event_log_lock_wait(log_lock_wait_ns);
        let dropped_count =
            *self.sdk_dropped_event_count.lock().expect("sdk_dropped_event_count mutex poisoned");
        let oldest_seq = log_guard.front().map(|entry| entry.seq_no);
        let latest_seq = log_guard.back().map(|entry| entry.seq_no);

        if cursor_is_expired(cursor_seq, oldest_seq) {
            let mut degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *degraded = true;
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_CURSOR_EXPIRED",
                "cursor is outside retained event window",
            ));
        }

        let start_seq = cursor_seq.map(|value| value.saturating_add(1)).or(oldest_seq).unwrap_or(0);
        let mut event_rows = Vec::new();
        let mut batch_bytes = 0_usize;
        let mut gap_seq = None;

        let append_event_row =
            |row: JsonValue, event_rows: &mut Vec<JsonValue>, batch_bytes: &mut usize| {
                let payload_bytes =
                    row.get("payload").map(|payload| payload.to_string().len()).unwrap_or(0);
                if payload_bytes > max_event_bytes {
                    return Err(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_EVENT_TOO_LARGE",
                        "event payload exceeds supported max_event_bytes limit",
                    ));
                }
                let extension_keys = row
                    .get("payload")
                    .and_then(|payload| payload.get("extensions"))
                    .and_then(JsonValue::as_object)
                    .map_or(0, JsonMap::len);
                if extension_keys > max_extension_keys {
                    return Err(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED",
                        "event extensions key count exceeds supported limit",
                    ));
                }
                let event_bytes = row.to_string().len();
                let next_batch_bytes = (*batch_bytes).saturating_add(event_bytes);
                if next_batch_bytes > max_batch_bytes {
                    return Err(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_BATCH_TOO_LARGE",
                        "event batch exceeds supported max_batch_bytes limit",
                    ));
                }
                *batch_bytes = next_batch_bytes;
                event_rows.push(row);
                Ok(())
            };

        if parsed.cursor.is_none() && event_rows.len() < parsed.max {
            let gap_meta = match compute_stream_gap(dropped_count, oldest_seq) {
                Ok(gap_meta) => gap_meta,
                Err(reason) => {
                    log::warn!("sdk poll: stream gap computation skipped: {reason}");
                    None
                }
            };
            if let Some(gap_meta) = gap_meta {
                gap_seq = Some(gap_meta.gap_seq_no);
                let gap_row = json!({
                        "event_id": format!("gap-{}", gap_meta.gap_seq_no),
                    "runtime_id": self.identity_hash,
                    "stream_id": SDK_STREAM_ID,
                        "seq_no": gap_meta.gap_seq_no,
                    "contract_version": self.active_contract_version(),
                    "ts_ms": (now_i64().max(0) as u64) * 1000,
                    "event_type": "StreamGap",
                    "severity": "warn",
                    "source_component": "rns-rpc",
                    "payload": {
                            "expected_seq_no": gap_meta.expected_seq_no,
                            "observed_seq_no": gap_meta.observed_seq_no,
                            "dropped_count": gap_meta.dropped_count,
                    },
                });
                if let Err(response) = append_event_row(gap_row, &mut event_rows, &mut batch_bytes)
                {
                    return Ok(response);
                }
            }
        }

        let delivery_destinations = self
            .service_identity_bridge()
            .map(|_| self.current_session_delivery_destinations());
        let remaining_slots = parsed.max.saturating_sub(event_rows.len());
        let mut last_scanned_seq = cursor_seq;
        if remaining_slots > 0 {
            for entry in log_guard
                .iter()
                .filter(|entry| entry.seq_no >= start_seq)
                .filter(|entry| entry.event.event_type != "sdk_lifecycle_trace")
            {
                last_scanned_seq = Some(entry.seq_no);
                if delivery_destinations.as_ref().is_some_and(|destinations| {
                    !self.event_visible_to_current_identity_session(&entry.event, destinations)
                }) {
                    continue;
                }
                let event_row = json!({
                    "event_id": format!("evt-{}", entry.seq_no),
                    "runtime_id": self.identity_hash,
                    "stream_id": SDK_STREAM_ID,
                    "seq_no": entry.seq_no,
                    "contract_version": self.active_contract_version(),
                    "ts_ms": (now_i64().max(0) as u64) * 1000,
                    "event_type": entry.event.event_type.clone(),
                    "severity": Self::event_severity(entry.event.event_type.as_str()),
                    "source_component": "rns-rpc",
                    "payload": entry.event.payload.clone(),
                });
                if let Err(response) =
                    append_event_row(event_row, &mut event_rows, &mut batch_bytes)
                {
                    return Ok(response);
                }
                if event_rows.len() >= parsed.max {
                    break;
                }
            }
        }

        let next_seq = last_scanned_seq
            .or(gap_seq)
            .or(latest_seq)
            .unwrap_or(0);
        let next_cursor = self.sdk_encode_cursor(next_seq);

        if clear_degraded_on_success {
            let mut degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *degraded = false;
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "events": event_rows,
                "next_cursor": next_cursor,
                "dropped_count": if parsed.cursor.is_none() { dropped_count } else { 0 },
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }
}
