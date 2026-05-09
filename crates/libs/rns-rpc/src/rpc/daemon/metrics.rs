use super::*;

impl RpcDaemon {
    pub(super) fn metrics_increment(map: &mut BTreeMap<String, u64>, key: &str) {
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

    pub(super) fn metrics_record_rpc_request(&self, method: &str) {
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

    pub(super) fn metrics_record_rpc_response(
        &self,
        method: &str,
        elapsed_ms: u64,
        response: &RpcResponse,
    ) {
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
                    metrics.sdk_send_success_total =
                        metrics.sdk_send_success_total.saturating_add(1);
                }
            }
            "sdk_poll_events_v2" => {
                metrics.sdk_poll_latency_ms.observe(elapsed_ms);
                if let Some(result) = response.result.as_ref() {
                    if let Some(events) = result.get("events").and_then(JsonValue::as_array) {
                        metrics.sdk_poll_events_total =
                            metrics.sdk_poll_events_total.saturating_add(events.len() as u64);
                        if events.iter().any(|event| {
                            event.get("event_type").and_then(JsonValue::as_str) == Some("StreamGap")
                        }) {
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

    pub(super) fn metrics_record_rpc_io_error(&self, method: &str, elapsed_ms: u64) {
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

    pub(super) fn metrics_record_auth_result(&self, elapsed_ms: u64, allowed: bool) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_auth_latency_ms.observe(elapsed_ms);
        if !allowed {
            metrics.sdk_auth_failures_total = metrics.sdk_auth_failures_total.saturating_add(1);
        }
    }

    pub(super) fn metrics_record_event_drop(&self) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_event_drops_total = metrics.sdk_event_drops_total.saturating_add(1);
    }

    pub(super) fn metrics_record_event_sink_skipped(&self) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_event_sink_skipped_total =
            metrics.sdk_event_sink_skipped_total.saturating_add(1);
    }

    pub(crate) fn metrics_record_sdk_send_store_write(&self, elapsed_ns: u64) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_send_store_write_ops_total =
            metrics.sdk_send_store_write_ops_total.saturating_add(1);
        metrics.sdk_send_store_write_ns_total =
            metrics.sdk_send_store_write_ns_total.saturating_add(elapsed_ns);
    }

    pub(crate) fn metrics_record_sdk_send_delivery_schedule(&self, elapsed_ns: u64) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_send_delivery_schedule_ops_total =
            metrics.sdk_send_delivery_schedule_ops_total.saturating_add(1);
        metrics.sdk_send_delivery_schedule_ns_total =
            metrics.sdk_send_delivery_schedule_ns_total.saturating_add(elapsed_ns);
    }

    pub(crate) fn metrics_record_sdk_send_event_publish(&self, elapsed_ns: u64) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_send_event_publish_ops_total =
            metrics.sdk_send_event_publish_ops_total.saturating_add(1);
        metrics.sdk_send_event_publish_ns_total =
            metrics.sdk_send_event_publish_ns_total.saturating_add(elapsed_ns);
    }

    pub fn metrics_record_ble_connect_failure(&self, iface: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.ble_connect_failures_total = metrics.ble_connect_failures_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.ble_connect_failures_by_iface, iface);
    }

    pub fn metrics_record_ble_chunk_retry(&self, iface: &str, reason: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.ble_chunk_retries_total = metrics.ble_chunk_retries_total.saturating_add(1);
        let key = format!("{iface}|{reason}");
        Self::metrics_increment(&mut metrics.ble_chunk_retries_by_iface_reason, key.as_str());
    }

    pub fn metrics_record_ble_nack(&self, iface: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.ble_nacks_total = metrics.ble_nacks_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.ble_nacks_by_iface, iface);
    }

    pub fn metrics_record_ble_tx_queue_timeout(&self, iface: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.ble_tx_queue_timeout_total = metrics.ble_tx_queue_timeout_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.ble_tx_queue_timeout_by_iface, iface);
    }

    pub fn metrics_record_attachment_upload_offset_reject(&self, code: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.attachment_upload_offset_reject_total =
            metrics.attachment_upload_offset_reject_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.attachment_upload_offset_reject_by_code, code);
    }

    pub fn metrics_record_attachment_upload_checksum_mismatch(&self) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.attachment_upload_checksum_mismatch_total =
            metrics.attachment_upload_checksum_mismatch_total.saturating_add(1);
    }

    pub fn metrics_record_capture_success(&self, camera_id: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.capture_success_total = metrics.capture_success_total.saturating_add(1);
        Self::metrics_increment(&mut metrics.capture_success_by_camera_id, camera_id);
    }

    pub fn metrics_record_capture_failure(&self, camera_id: &str, reason: &str) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.capture_failure_total = metrics.capture_failure_total.saturating_add(1);
        let key = format!("{camera_id}|{reason}");
        Self::metrics_increment(&mut metrics.capture_failure_by_camera_reason, key.as_str());
    }

    pub(crate) fn metrics_record_daemon_status_wait(
        &self,
        snapshot_wait_ns: u64,
        message_count_wait_ns: u64,
    ) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.daemon_status_calls_total = metrics.daemon_status_calls_total.saturating_add(1);
        metrics.daemon_status_snapshot_wait_ns_total =
            metrics.daemon_status_snapshot_wait_ns_total.saturating_add(snapshot_wait_ns);
        metrics.daemon_status_message_count_wait_ns_total =
            metrics.daemon_status_message_count_wait_ns_total.saturating_add(message_count_wait_ns);
        metrics.daemon_status_lock_wait_ns_total = metrics
            .daemon_status_lock_wait_ns_total
            .saturating_add(snapshot_wait_ns.saturating_add(message_count_wait_ns));
    }

    pub(crate) fn metrics_record_sdk_poll_event_log_lock_wait(&self, wait_ns: u64) {
        let mut metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned");
        metrics.sdk_poll_event_log_lock_ops_total =
            metrics.sdk_poll_event_log_lock_ops_total.saturating_add(1);
        metrics.sdk_poll_event_log_lock_wait_ns_total =
            metrics.sdk_poll_event_log_lock_wait_ns_total.saturating_add(wait_ns);
    }

    pub fn metrics_snapshot(&self) -> JsonValue {
        let metrics = self.sdk_metrics.lock().expect("sdk_metrics mutex poisoned").clone();
        let event_queue_depth = self.event_queue.lock().expect("event_queue mutex poisoned").len();
        let sdk_event_log_depth =
            self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned").len();
        let dropped_count =
            *self.sdk_dropped_event_count.lock().expect("sdk_dropped_event_count mutex poisoned");
        let store_contention = self.store.contention_snapshot();
        let mut counters = JsonMap::new();
        counters.insert("http_requests_total".to_string(), json!(metrics.http_requests_total));
        counters.insert(
            "http_request_errors_total".to_string(),
            json!(metrics.http_request_errors_total),
        );
        counters.insert("rpc_requests_total".to_string(), json!(metrics.rpc_requests_total));
        counters.insert("rpc_errors_total".to_string(), json!(metrics.rpc_errors_total));
        counters.insert("sdk_send_total".to_string(), json!(metrics.sdk_send_total));
        counters
            .insert("sdk_send_success_total".to_string(), json!(metrics.sdk_send_success_total));
        counters.insert("sdk_send_error_total".to_string(), json!(metrics.sdk_send_error_total));
        counters.insert(
            "sdk_send_store_write_ops_total".to_string(),
            json!(metrics.sdk_send_store_write_ops_total),
        );
        counters.insert(
            "sdk_send_store_write_ns_total".to_string(),
            json!(metrics.sdk_send_store_write_ns_total),
        );
        counters.insert(
            "sdk_send_delivery_schedule_ops_total".to_string(),
            json!(metrics.sdk_send_delivery_schedule_ops_total),
        );
        counters.insert(
            "sdk_send_delivery_schedule_ns_total".to_string(),
            json!(metrics.sdk_send_delivery_schedule_ns_total),
        );
        counters.insert(
            "sdk_send_event_publish_ops_total".to_string(),
            json!(metrics.sdk_send_event_publish_ops_total),
        );
        counters.insert(
            "sdk_send_event_publish_ns_total".to_string(),
            json!(metrics.sdk_send_event_publish_ns_total),
        );
        counters.insert("sdk_poll_total".to_string(), json!(metrics.sdk_poll_total));
        counters.insert("sdk_poll_events_total".to_string(), json!(metrics.sdk_poll_events_total));
        counters.insert(
            "sdk_poll_batches_with_gap_total".to_string(),
            json!(metrics.sdk_poll_batches_with_gap_total),
        );
        counters.insert("sdk_cancel_total".to_string(), json!(metrics.sdk_cancel_total));
        counters.insert(
            "sdk_cancel_accepted_total".to_string(),
            json!(metrics.sdk_cancel_accepted_total),
        );
        counters.insert(
            "sdk_cancel_too_late_total".to_string(),
            json!(metrics.sdk_cancel_too_late_total),
        );
        counters.insert(
            "sdk_cancel_not_found_total".to_string(),
            json!(metrics.sdk_cancel_not_found_total),
        );
        counters.insert(
            "sdk_cancel_already_terminal_total".to_string(),
            json!(metrics.sdk_cancel_already_terminal_total),
        );
        counters.insert("sdk_event_drops_total".to_string(), json!(metrics.sdk_event_drops_total));
        counters.insert(
            "sdk_event_sink_publish_total".to_string(),
            json!(metrics.sdk_event_sink_publish_total),
        );
        counters.insert(
            "sdk_event_sink_error_total".to_string(),
            json!(metrics.sdk_event_sink_error_total),
        );
        counters.insert(
            "sdk_event_sink_skipped_total".to_string(),
            json!(metrics.sdk_event_sink_skipped_total),
        );
        counters
            .insert("sdk_auth_failures_total".to_string(), json!(metrics.sdk_auth_failures_total));
        counters.insert("sdk_event_dropped_count".to_string(), json!(dropped_count));
        counters.insert(
            "ble_connect_failures_total".to_string(),
            json!(metrics.ble_connect_failures_total),
        );
        counters
            .insert("ble_chunk_retries_total".to_string(), json!(metrics.ble_chunk_retries_total));
        counters.insert("ble_nacks_total".to_string(), json!(metrics.ble_nacks_total));
        counters.insert(
            "ble_tx_queue_timeout_total".to_string(),
            json!(metrics.ble_tx_queue_timeout_total),
        );
        counters.insert(
            "attachment_upload_offset_reject_total".to_string(),
            json!(metrics.attachment_upload_offset_reject_total),
        );
        counters.insert(
            "attachment_upload_checksum_mismatch_total".to_string(),
            json!(metrics.attachment_upload_checksum_mismatch_total),
        );
        counters.insert("capture_success_total".to_string(), json!(metrics.capture_success_total));
        counters.insert("capture_failure_total".to_string(), json!(metrics.capture_failure_total));
        counters.insert(
            "daemon_status_calls_total".to_string(),
            json!(metrics.daemon_status_calls_total),
        );
        counters.insert(
            "daemon_status_lock_wait_ns_total".to_string(),
            json!(metrics.daemon_status_lock_wait_ns_total),
        );
        counters.insert(
            "daemon_status_snapshot_wait_ns_total".to_string(),
            json!(metrics.daemon_status_snapshot_wait_ns_total),
        );
        counters.insert(
            "daemon_status_message_count_wait_ns_total".to_string(),
            json!(metrics.daemon_status_message_count_wait_ns_total),
        );
        counters.insert(
            "sdk_poll_event_log_lock_ops_total".to_string(),
            json!(metrics.sdk_poll_event_log_lock_ops_total),
        );
        counters.insert(
            "sdk_poll_event_log_lock_wait_ns_total".to_string(),
            json!(metrics.sdk_poll_event_log_lock_wait_ns_total),
        );

        json!({
            "runtime_id": self.identity_hash,
            "counters": counters,
            "depth": {
                "legacy_event_queue_depth": event_queue_depth,
                "sdk_event_log_depth": sdk_event_log_depth,
            },
            "http_requests_by_route": metrics.http_requests_by_route,
            "rpc_requests_by_method": metrics.rpc_requests_by_method,
            "rpc_errors_by_method": metrics.rpc_errors_by_method,
            "sdk_event_sink_publish_by_kind": metrics.sdk_event_sink_publish_by_kind,
            "sdk_event_sink_errors_by_kind": metrics.sdk_event_sink_errors_by_kind,
            "ble_connect_failures_by_iface": metrics.ble_connect_failures_by_iface,
            "ble_chunk_retries_by_iface_reason": metrics.ble_chunk_retries_by_iface_reason,
            "ble_nacks_by_iface": metrics.ble_nacks_by_iface,
            "ble_tx_queue_timeout_by_iface": metrics.ble_tx_queue_timeout_by_iface,
            "attachment_upload_offset_reject_by_code": metrics.attachment_upload_offset_reject_by_code,
            "capture_success_by_camera_id": metrics.capture_success_by_camera_id,
            "capture_failure_by_camera_reason": metrics.capture_failure_by_camera_reason,
            "histograms": {
                "sdk_send_latency_ms": metrics.sdk_send_latency_ms.as_json(),
                "sdk_poll_latency_ms": metrics.sdk_poll_latency_ms.as_json(),
                "sdk_auth_latency_ms": metrics.sdk_auth_latency_ms.as_json(),
            },
            "storage": {
                "read_ops_total": store_contention.read_ops_total,
                "read_lock_wait_ns_total": store_contention.read_lock_wait_ns_total,
                "write_ops_total": store_contention.write_ops_total,
                "write_lock_wait_ns_total": store_contention.write_lock_wait_ns_total,
            },
            "meta": self.response_meta(),
        })
    }
}
