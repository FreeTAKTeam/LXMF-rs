impl RpcDaemon {
    fn metrics_increment(map: &mut BTreeMap<String, u64>, key: &str) {
        let count = map.entry(key.to_string()).or_insert(0);
        *count = count.saturating_add(1);
    }

    pub(crate) fn metrics_record_http_request(&self, method: &str, path: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.http_requests_total = metrics.http_requests_total.saturating_add(1);
        Self::metrics_increment(
            &mut metrics.http_requests_by_route,
            format!("{method} {path}").as_str(),
        );
    }

    pub(crate) fn metrics_record_http_error(&self) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.http_request_errors_total = metrics.http_request_errors_total.saturating_add(1);
    }

    fn metrics_record_rpc_request(&self, method: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.rpc_requests_total = metrics.rpc_requests_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.rpc_requests_by_method, method);
        match method {
            "sdk_send_v2" | "send_message" | "send_message_v2" => {
                metrics.sdk_send_total = metrics.sdk_send_total.saturating_add(1);
            }
            "sdk_poll_events_v2" => {
                metrics.sdk_poll_total = metrics.sdk_poll_total.saturating_add(1);
            }
            "sdk_cancel_message_v2" => {
                metrics.sdk_cancel_total = metrics.sdk_cancel_total.saturating_add(1);
            }
            _ => {}
        }
    }

    fn metrics_record_rpc_response(&self, method: &str, elapsed_ms: u64, response: &RpcResponse) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        if response.error.is_some() {
            metrics.rpc_errors_total = metrics.rpc_errors_total.saturating_add(1);
            Self::metrics_increment(&mut metrics.rpc_errors_by_method, method);
        }
        match method {
            "sdk_send_v2" | "send_message" | "send_message_v2" => {
                metrics.sdk_send_latency_ms.observe(elapsed_ms);
                if response.error.is_some() {
                    metrics.sdk_send_error_total = metrics.sdk_send_error_total.saturating_add(1);
                } else {
                    metrics.sdk_send_success_total = metrics.sdk_send_success_total.saturating_add(1);
                }
            }
            "sdk_poll_events_v2" => {
                metrics.sdk_poll_latency_ms.observe(elapsed_ms);
                if let Some(result) = response.result.as_ref() {
                    if let Some(events) = result.get("events").and_then(JsonValue::as_array) {
                        metrics.sdk_poll_events_total = metrics
                            .sdk_poll_events_total
                            .saturating_add(events.len() as u64);
                        if events
                            .iter()
                            .any(|event| event.get("event_type").and_then(JsonValue::as_str)
                                == Some("StreamGap"))
                        {
                            metrics.sdk_poll_batches_with_gap_total =
                                metrics.sdk_poll_batches_with_gap_total.saturating_add(1);
                        }
                    }
                }
            }
            "sdk_cancel_message_v2" => {
                if let Some(result) = response.result.as_ref() {
                    let outcome = result.get("result").and_then(JsonValue::as_str).unwrap_or("");
                    match outcome {
                        "Accepted" => {
                            metrics.sdk_cancel_accepted_total =
                                metrics.sdk_cancel_accepted_total.saturating_add(1);
                        }
                        "TooLateToCancel" => {
                            metrics.sdk_cancel_too_late_total =
                                metrics.sdk_cancel_too_late_total.saturating_add(1);
                        }
                        "AlreadyTerminal" => {
                            metrics.sdk_cancel_already_terminal_total =
                                metrics.sdk_cancel_already_terminal_total.saturating_add(1);
                        }
                        "NotFound" => {
                            metrics.sdk_cancel_not_found_total =
                                metrics.sdk_cancel_not_found_total.saturating_add(1);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn metrics_record_rpc_io_error(&self, method: &str, elapsed_ms: u64) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.rpc_errors_total = metrics.rpc_errors_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.rpc_errors_by_method, method);
        match method {
            "sdk_send_v2" | "send_message" | "send_message_v2" => {
                metrics.sdk_send_error_total = metrics.sdk_send_error_total.saturating_add(1);
                metrics.sdk_send_latency_ms.observe(elapsed_ms);
            }
            "sdk_poll_events_v2" => {
                metrics.sdk_poll_latency_ms.observe(elapsed_ms);
            }
            _ => {}
        }
    }

    fn metrics_record_auth_result(&self, elapsed_ms: u64, allowed: bool) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_auth_latency_ms.observe(elapsed_ms);
        if !allowed {
            metrics.sdk_auth_failures_total = metrics.sdk_auth_failures_total.saturating_add(1);
        }
    }

    fn metrics_record_event_drop(&self) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_event_drops_total = metrics.sdk_event_drops_total.saturating_add(1);
    }

    fn metrics_record_event_sink_publish(&self, kind: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_event_sink_publish_total = metrics.sdk_event_sink_publish_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.sdk_event_sink_publish_by_kind, kind);
    }

    fn metrics_record_event_sink_error(&self, kind: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_event_sink_error_total = metrics.sdk_event_sink_error_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.sdk_event_sink_errors_by_kind, kind);
    }

    fn metrics_record_event_sink_skipped(&self) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_event_sink_skipped_total = metrics.sdk_event_sink_skipped_total.saturating_add(1);
    }

    pub fn metrics_snapshot(&self) -> JsonValue {
        let metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned").clone();
        let event_queue_depth = self.event_queue.lock().expect("event_queue mutex poisoned").len();
        let sdk_event_log_depth =
            self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned").len();
        let dropped_count = *self
            .sdk_dropped_event_count
            .lock()
            .expect("sdk_dropped_event_count mutex poisoned");

        json!({
            "runtime_id": self.identity_hash,
            "counters": {
                "http_requests_total": metrics.http_requests_total,
                "http_request_errors_total": metrics.http_request_errors_total,
                "rpc_requests_total": metrics.rpc_requests_total,
                "rpc_errors_total": metrics.rpc_errors_total,
                "sdk_send_total": metrics.sdk_send_total,
                "sdk_send_success_total": metrics.sdk_send_success_total,
                "sdk_send_error_total": metrics.sdk_send_error_total,
                "sdk_poll_total": metrics.sdk_poll_total,
                "sdk_poll_events_total": metrics.sdk_poll_events_total,
                "sdk_poll_batches_with_gap_total": metrics.sdk_poll_batches_with_gap_total,
                "sdk_cancel_total": metrics.sdk_cancel_total,
                "sdk_cancel_accepted_total": metrics.sdk_cancel_accepted_total,
                "sdk_cancel_too_late_total": metrics.sdk_cancel_too_late_total,
                "sdk_cancel_not_found_total": metrics.sdk_cancel_not_found_total,
                "sdk_cancel_already_terminal_total": metrics.sdk_cancel_already_terminal_total,
                "sdk_event_drops_total": metrics.sdk_event_drops_total,
                "sdk_event_sink_publish_total": metrics.sdk_event_sink_publish_total,
                "sdk_event_sink_error_total": metrics.sdk_event_sink_error_total,
                "sdk_event_sink_skipped_total": metrics.sdk_event_sink_skipped_total,
                "sdk_auth_failures_total": metrics.sdk_auth_failures_total,
                "sdk_event_dropped_count": dropped_count,
            },
            "depth": {
                "legacy_event_queue_depth": event_queue_depth,
                "sdk_event_log_depth": sdk_event_log_depth,
            },
            "http_requests_by_route": metrics.http_requests_by_route,
            "rpc_requests_by_method": metrics.rpc_requests_by_method,
            "rpc_errors_by_method": metrics.rpc_errors_by_method,
            "sdk_event_sink_publish_by_kind": metrics.sdk_event_sink_publish_by_kind,
            "sdk_event_sink_errors_by_kind": metrics.sdk_event_sink_errors_by_kind,
            "histograms": {
                "sdk_send_latency_ms": metrics.sdk_send_latency_ms.as_json(),
                "sdk_poll_latency_ms": metrics.sdk_poll_latency_ms.as_json(),
                "sdk_auth_latency_ms": metrics.sdk_auth_latency_ms.as_json(),
            },
            "meta": self.response_meta(),
        })
    }
}
