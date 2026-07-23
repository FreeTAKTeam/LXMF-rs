use super::*;

impl RpcDaemon {
    pub fn handle_rpc(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let request_id = request.id;
        let method = request.method.clone();
        let metrics_started = std::time::Instant::now();
        self.metrics_record_rpc_request(method.as_str());
        let is_sdk_method = method.starts_with("sdk_");
        let trace_lifecycle = Self::should_trace_sdk_lifecycle(method.as_str());
        let lifecycle_trace_id =
            trace_lifecycle.then(|| Self::sdk_lifecycle_trace_id(method.as_str(), request_id));
        if let Some(trace_id) = lifecycle_trace_id.as_deref() {
            let mut details = JsonMap::new();
            details.insert("is_sdk_method".to_string(), JsonValue::Bool(is_sdk_method));
            self.emit_sdk_lifecycle_trace(
                trace_id,
                request_id,
                method.as_str(),
                "start",
                "pending",
                details,
            );
        }
        let response = match method.as_str() {
            "status" => Ok(RpcResponse {
                id: request.id,
                result: Some(self.daemon_status_result(false)?),
                error: None,
            }),
            "sdk_negotiate_v2" => self.handle_sdk_negotiate_v2(request),
            "daemon_status_ex" => Ok(RpcResponse {
                id: request.id,
                result: Some(self.daemon_status_result(true)?),
                error: None,
            }),
            "sdk_snapshot_v2" => self.handle_sdk_snapshot_v2(request),
            "sdk_status_v2" => self.handle_sdk_status_v2(request),
            "sdk_cursor_hint_v2" => self.handle_sdk_cursor_hint_v2(request),
            "sdk_configure_v2" => self.handle_sdk_configure_v2(request),
            "sdk_shutdown_v2" => self.handle_sdk_shutdown_v2(request),
            "sdk_topic_create_v2" => self.handle_sdk_topic_create_v2(request),
            "sdk_topic_get_v2" => self.handle_sdk_topic_get_v2(request),
            "sdk_topic_list_v2" => self.handle_sdk_topic_list_v2(request),
            "sdk_topic_subscribe_v2" => self.handle_sdk_topic_subscribe_v2(request),
            "sdk_topic_unsubscribe_v2" => self.handle_sdk_topic_unsubscribe_v2(request),
            "sdk_topic_publish_v2" => self.handle_sdk_topic_publish_v2(request),
            "sdk_telemetry_query_v2" => self.handle_sdk_telemetry_query_v2(request),
            "sdk_telemetry_subscribe_v2" => self.handle_sdk_telemetry_subscribe_v2(request),
            "sdk_attachment_store_v2" => self.handle_sdk_attachment_store_v2(request),
            "sdk_attachment_get_v2" => self.handle_sdk_attachment_get_v2(request),
            "sdk_attachment_list_v2" => self.handle_sdk_attachment_list_v2(request),
            "sdk_attachment_delete_v2" => self.handle_sdk_attachment_delete_v2(request),
            "sdk_attachment_download_v2" => self.handle_sdk_attachment_download_v2(request),
            "sdk_attachment_upload_start_v2" => self.handle_sdk_attachment_upload_start_v2(request),
            "sdk_attachment_upload_chunk_v2" => self.handle_sdk_attachment_upload_chunk_v2(request),
            "sdk_attachment_upload_commit_v2" => {
                self.handle_sdk_attachment_upload_commit_v2(request)
            }
            "sdk_attachment_download_chunk_v2" => {
                self.handle_sdk_attachment_download_chunk_v2(request)
            }
            "sdk_attachment_associate_topic_v2" => {
                self.handle_sdk_attachment_associate_topic_v2(request)
            }
            "sdk_marker_create_v2" => self.handle_sdk_marker_create_v2(request),
            "sdk_marker_list_v2" => self.handle_sdk_marker_list_v2(request),
            "sdk_marker_update_position_v2" => self.handle_sdk_marker_update_position_v2(request),
            "sdk_marker_delete_v2" => self.handle_sdk_marker_delete_v2(request),
            "sdk_identity_list_v2" => self.handle_sdk_identity_list_v2(request),
            "sdk_identity_create_v2" => self.handle_sdk_identity_create_v2(request),
            "sdk_identity_announce_now_v2" => self.handle_sdk_identity_announce_now_v2(request),
            "sdk_identity_presence_list_v2" => self.handle_sdk_identity_presence_list_v2(request),
            "sdk_identity_activate_v2" => self.handle_sdk_identity_activate_v2(request),
            "sdk_identity_import_v2" => self.handle_sdk_identity_import_v2(request),
            "sdk_identity_export_v2" => self.handle_sdk_identity_export_v2(request),
            "sdk_identity_resolve_v2" => self.handle_sdk_identity_resolve_v2(request),
            "sdk_identity_contact_update_v2" => self.handle_sdk_identity_contact_update_v2(request),
            "sdk_identity_contact_list_v2" => self.handle_sdk_identity_contact_list_v2(request),
            "sdk_identity_bootstrap_v2" => self.handle_sdk_identity_bootstrap_v2(request),
            "sdk_peer_connect_v2" => self.handle_sdk_peer_connect_v2(request),
            "sdk_peer_disconnect_v2" => self.handle_sdk_peer_disconnect_v2(request),
            "sdk_peer_reconnect_v2" => self.handle_sdk_peer_reconnect_v2(request),
            "sdk_workflow_peer_ready_v2" => self.handle_sdk_workflow_peer_ready_v2(request),
            "sdk_workflow_topic_sync_v2" => self.handle_sdk_workflow_topic_sync_v2(request),
            "sdk_workflow_attachment_report_publish_v2" => {
                self.handle_sdk_workflow_attachment_report_publish_v2(request)
            }
            "sdk_workflow_mission_update_send_v2" => {
                self.handle_sdk_workflow_mission_update_send_v2(request)
            }
            "sdk_paper_encode_v2" => self.handle_sdk_paper_encode_v2(request),
            "sdk_paper_decode_v2" => self.handle_sdk_paper_decode_v2(request),
            "sdk_operation_registry_v2" => self.handle_sdk_operation_registry_v2(request),
            "sdk_envelope_execute_v2" => self.handle_sdk_envelope_execute_v2(request),
            "sdk_command_invoke_v2" => self.handle_sdk_command_invoke_v2(request),
            "sdk_command_reply_v2" => self.handle_sdk_command_reply_v2(request),
            "sdk_command_session_get_v2" => self.handle_sdk_command_session_get_v2(request),
            "sdk_command_session_list_v2" => self.handle_sdk_command_session_list_v2(request),
            "sdk_voice_session_open_v2" => self.handle_sdk_voice_session_open_v2(request),
            "sdk_voice_session_update_v2" => self.handle_sdk_voice_session_update_v2(request),
            "sdk_voice_session_close_v2" => self.handle_sdk_voice_session_close_v2(request),
            _ => self.handle_rpc_legacy(request),
        };

        match response {
            Ok(response) => {
                self.record_sdk_cursor_hint(method.as_str(), &response);
                let elapsed_ms = metrics_started.elapsed().as_millis() as u64;
                self.metrics_record_rpc_response(method.as_str(), elapsed_ms, &response);
                if let Some(trace_id) = lifecycle_trace_id.as_deref() {
                    let details = Self::sdk_lifecycle_details(method.as_str(), &response);
                    let outcome = if response.error.is_some() { "error" } else { "ok" };
                    self.emit_sdk_lifecycle_trace(
                        trace_id,
                        request_id,
                        method.as_str(),
                        "finish",
                        outcome,
                        details,
                    );
                }
                Ok(response)
            }
            Err(error) if is_sdk_method && error.kind() == std::io::ErrorKind::InvalidInput => {
                let message = error.to_string();
                let normalized = message.to_ascii_lowercase();
                let (code, message) = if normalized.contains("unknown field") {
                    ("SDK_VALIDATION_UNKNOWN_FIELD", "request contains unknown fields")
                } else {
                    ("SDK_VALIDATION_INVALID_ARGUMENT", message.as_str())
                };
                let mapped = self.sdk_error_response(request_id, code, message);
                let elapsed_ms = metrics_started.elapsed().as_millis() as u64;
                self.metrics_record_rpc_response(method.as_str(), elapsed_ms, &mapped);
                if let Some(trace_id) = lifecycle_trace_id.as_deref() {
                    let mut details = Self::sdk_lifecycle_details(method.as_str(), &mapped);
                    details.insert("mapped_invalid_input".to_string(), JsonValue::Bool(true));
                    self.emit_sdk_lifecycle_trace(
                        trace_id,
                        request_id,
                        method.as_str(),
                        "finish",
                        "error",
                        details,
                    );
                }
                Ok(mapped)
            }
            Err(error) => {
                let elapsed_ms = metrics_started.elapsed().as_millis() as u64;
                self.metrics_record_rpc_io_error(method.as_str(), elapsed_ms);
                if let Some(trace_id) = lifecycle_trace_id.as_deref() {
                    let mut details = JsonMap::new();
                    details.insert(
                        "io_error_kind".to_string(),
                        JsonValue::String(format!("{:?}", error.kind())),
                    );
                    details.insert("io_error".to_string(), JsonValue::String(error.to_string()));
                    self.emit_sdk_lifecycle_trace(
                        trace_id,
                        request_id,
                        method.as_str(),
                        "finish",
                        "error",
                        details,
                    );
                }
                Err(error)
            }
        }
    }

    pub(super) fn daemon_status_result(
        &self,
        record_wait_metrics: bool,
    ) -> Result<JsonValue, std::io::Error> {
        let snapshot_started = std::time::Instant::now();
        let snapshot = self.daemon_status_snapshot();
        let snapshot_wait_ns =
            snapshot_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let message_count_started = std::time::Instant::now();
        let message_count = self.store.message_count().map_err(std::io::Error::other)? as usize;
        let message_count_wait_ns =
            message_count_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        if record_wait_metrics {
            self.metrics_record_daemon_status_wait(snapshot_wait_ns, message_count_wait_ns);
        }

        Ok(json!({
            "identity_hash": self.identity_hash,
            "delivery_destination_hash": self.local_delivery_hash(),
            "running": true,
            "peer_count": snapshot.peer_count,
            "message_count": message_count,
            "interface_count": snapshot.interfaces.len(),
            "interfaces": snapshot.interfaces,
            "reticulum": {
                "shared_instance": Self::shared_instance_status(&snapshot.interfaces),
            },
            "delivery_policy": snapshot.delivery_policy,
            "propagation": snapshot.propagation,
            "stamp_policy": snapshot.stamp_policy,
            "delivery_pipeline": self.outbound_bridge.as_ref().and_then(|bridge| bridge.delivery_pipeline_status()),
            "capabilities": Self::capabilities(),
        }))
    }

    fn shared_instance_status(interfaces: &[InterfaceRecord]) -> JsonValue {
        let mut selected: Option<(usize, &InterfaceRecord)> = None;
        for record in interfaces {
            let priority = shared_instance_priority(record);
            if priority == 0 {
                continue;
            }
            if selected.is_none_or(|(current, _)| priority > current) {
                selected = Some((priority, record));
            }
        }

        let Some((_priority, record)) = selected else {
            return json!({
                "mode": "disabled",
                "share_instance": false,
                "is_shared_instance": false,
                "is_connected_to_shared_instance": false,
                "shared_instance_type": JsonValue::Null,
                "endpoint": JsonValue::Null,
                "interface_name": JsonValue::Null,
            });
        };

        let startup_status = interface_runtime_str(record, "startup_status");
        let is_shared_instance = record.kind == "local" && startup_status == Some("active");
        let is_connected_to_shared_instance = startup_status == Some("attached");
        let configured = is_shared_instance_candidate(record);
        let mode = if is_shared_instance {
            "server"
        } else if is_connected_to_shared_instance {
            "client"
        } else if configured {
            "configured"
        } else {
            "disabled"
        };
        let shared_instance_type =
            interface_setting_str(record, "shared_instance_type").unwrap_or("tcp");

        json!({
            "mode": mode,
            "share_instance": is_shared_instance,
            "is_shared_instance": is_shared_instance,
            "is_connected_to_shared_instance": is_connected_to_shared_instance,
            "shared_instance_type": shared_instance_type,
            "endpoint": shared_instance_endpoint(record, shared_instance_type),
            "interface_name": record.name.clone(),
        })
    }

    pub(super) fn append_delivery_trace_to(
        traces: &Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>,
        message_id: &str,
        status: String,
    ) {
        const MAX_DELIVERY_TRACE_ENTRIES: usize = 32;
        const MAX_TRACKED_MESSAGE_TRACES: usize = 2048;

        let timestamp = now_i64();
        let reason_code = delivery_reason_code(&status).map(ToOwned::to_owned);
        let mut guard = traces.lock().expect("delivery traces mutex poisoned");
        let entry = guard.entry(message_id.to_string()).or_default();
        entry.push(DeliveryTraceEntry { status, timestamp, reason_code });
        if entry.len() > MAX_DELIVERY_TRACE_ENTRIES {
            let drain_count = entry.len().saturating_sub(MAX_DELIVERY_TRACE_ENTRIES);
            entry.drain(0..drain_count);
        }

        if guard.len() > MAX_TRACKED_MESSAGE_TRACES {
            let overflow = guard.len() - MAX_TRACKED_MESSAGE_TRACES;
            let mut evicted_ids = Vec::with_capacity(overflow);
            for key in guard.keys() {
                if key != message_id {
                    evicted_ids.push(key.clone());
                    if evicted_ids.len() == overflow {
                        break;
                    }
                }
            }
            for id in evicted_ids {
                guard.remove(&id);
            }

            if guard.len() > MAX_TRACKED_MESSAGE_TRACES {
                let still_over = guard.len() - MAX_TRACKED_MESSAGE_TRACES;
                let mut fallback = Vec::with_capacity(still_over);
                for key in guard.keys().take(still_over).cloned() {
                    fallback.push(key);
                }
                for id in fallback {
                    guard.remove(&id);
                }
            }
        }
    }

    pub(super) fn append_delivery_trace(&self, message_id: &str, status: String) {
        Self::append_delivery_trace_to(&self.delivery_traces, message_id, status);
    }
}

fn shared_instance_priority(record: &InterfaceRecord) -> usize {
    if !is_shared_instance_candidate(record) {
        return 0;
    }

    match interface_runtime_str(record, "startup_status") {
        Some("active") if record.kind == "local" => 4,
        Some("attached") => 3,
        None if record.enabled => 2,
        Some("failed") if record.enabled => 1,
        _ => 0,
    }
}

fn is_shared_instance_candidate(record: &InterfaceRecord) -> bool {
    record.kind == "local" || record.kind == "local_client"
}

fn interface_runtime_str<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
    record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get(key))
        .and_then(JsonValue::as_str)
}

fn interface_setting<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a JsonValue> {
    record.settings.as_ref().and_then(|settings| settings.get(key))
}

fn interface_setting_str<'a>(record: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
    interface_setting(record, key).and_then(JsonValue::as_str)
}

fn interface_setting_u64(record: &InterfaceRecord, key: &str) -> Option<u64> {
    interface_setting(record, key).and_then(JsonValue::as_u64)
}

fn shared_instance_endpoint(record: &InterfaceRecord, shared_instance_type: &str) -> JsonValue {
    match shared_instance_type {
        "unix" => interface_setting_str(record, "socket_path")
            .or_else(|| interface_setting_str(record, "instance_name"))
            .map_or(JsonValue::Null, |endpoint| json!(endpoint)),
        _ => {
            let Some(port) =
                record.port.map(u64::from).or_else(|| interface_setting_u64(record, "port"))
            else {
                return JsonValue::Null;
            };
            let host = record
                .host
                .as_deref()
                .or_else(|| interface_setting_str(record, "host"))
                .unwrap_or(if record.kind == "local" { "0.0.0.0" } else { "127.0.0.1" });
            json!(format!("tcp://{host}:{port}"))
        }
    }
}
