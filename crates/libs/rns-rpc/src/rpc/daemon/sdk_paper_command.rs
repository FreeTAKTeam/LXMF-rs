use super::*;

type InboundSdkCommandUpdate = (
    String,
    &'static str,
    Option<bool>,
    Option<JsonValue>,
    JsonMap<String, JsonValue>,
    Option<String>,
    Option<&'static str>,
);

impl RpcDaemon {
    pub(super) fn sdk_command_event_payload_summary(payload: &JsonValue) -> JsonValue {
        let byte_len = payload.to_string().len();
        match payload {
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => payload.clone(),
            JsonValue::String(value) if byte_len <= 512 => JsonValue::String(value.clone()),
            JsonValue::String(value) => {
                let preview = value.chars().take(128).collect::<String>();
                json!({
                    "kind": "string",
                    "byte_len": byte_len,
                    "preview": preview,
                    "truncated": true,
                })
            }
            JsonValue::Array(items) => json!({
                "kind": "array",
                "len": items.len(),
                "byte_len": byte_len,
            }),
            JsonValue::Object(map) => json!({
                "kind": "object",
                "keys": map.keys().take(16).cloned().collect::<Vec<_>>(),
                "byte_len": byte_len,
                "truncated": byte_len > 512,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_sdk_command_update(
        &self,
        correlation_id: &str,
        event_type: &str,
        accepted: Option<bool>,
        payload: Option<JsonValue>,
        extensions: JsonMap<String, JsonValue>,
        delivery_state: Option<String>,
        command_state: Option<&str>,
    ) -> Result<Option<SdkRemoteCommandRecord>, std::io::Error> {
        let now_ms = u64::try_from(now_i64()).unwrap_or(0);
        let updated_session = {
            let mut sessions =
                self.sdk_remote_commands.lock().expect("sdk_remote_commands mutex poisoned");
            let Some(session) = sessions.get_mut(correlation_id) else {
                return Ok(None);
            };
            session.updated_at_ms = now_ms;
            if let Some(accepted) = accepted {
                session.accepted = Some(accepted);
            }
            if let Some(payload) = payload {
                session.response_payload = Some(payload);
            }
            if let Some(delivery_state) = delivery_state {
                session.delivery_state = Some(delivery_state);
            }
            if let Some(command_state) = command_state {
                session.command_state = command_state.to_owned();
            }
            session.extensions.extend(extensions);
            session.clone()
        };
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: event_type.into(),
            payload: json!({
                "command_id": updated_session.command_id,
                "correlation_id": updated_session.correlation_id,
                "command": updated_session.command,
                "target": updated_session.target,
                "delivery_state": updated_session.delivery_state,
                "command_state": updated_session.command_state,
                "accepted": updated_session.accepted,
                "response_payload": updated_session
                    .response_payload
                    .as_ref()
                    .map(Self::sdk_command_event_payload_summary)
                    .unwrap_or(JsonValue::Null),
            }),
        });
        Ok(Some(updated_session))
    }

    pub(super) fn inbound_sdk_command_update(
        record: &MessageRecord,
    ) -> Option<InboundSdkCommandUpdate> {
        let fields = record.fields.as_ref()?.as_object()?;
        let command = fields.get("sdk_command")?.as_object()?;
        let correlation_id = Self::normalize_non_empty(command.get("correlation_id")?.as_str()?)?;
        let event = Self::normalize_non_empty(command.get("event")?.as_str()?)?;
        let accepted = command.get("accepted").and_then(JsonValue::as_bool);
        let payload = command.get("payload").cloned();
        let extensions =
            command.get("extensions").and_then(JsonValue::as_object).cloned().unwrap_or_default();
        match event.as_str() {
            "receipt_acknowledged" => Some((
                correlation_id,
                "command.receipt_acknowledged",
                accepted,
                payload,
                extensions,
                Some("acknowledged".to_owned()),
                None,
            )),
            "processing_started" => Some((
                correlation_id,
                "command.processing_started",
                accepted,
                payload,
                extensions,
                None,
                Some("processing"),
            )),
            "progress" => Some((
                correlation_id,
                "command.progress",
                accepted,
                payload,
                extensions,
                None,
                Some("processing"),
            )),
            "completed" => Some((
                correlation_id,
                "command.completed",
                Some(accepted.unwrap_or(true)),
                payload,
                extensions,
                None,
                Some("completed"),
            )),
            "failed" => Some((
                correlation_id,
                "command.failed",
                Some(accepted.unwrap_or(false)),
                payload,
                extensions,
                None,
                Some("failed"),
            )),
            _ => None,
        }
    }

    pub(crate) fn correlate_inbound_sdk_command(
        &self,
        record: &MessageRecord,
    ) -> Result<bool, std::io::Error> {
        let Some((
            correlation_id,
            event_type,
            accepted,
            payload,
            extensions,
            delivery_state,
            command_state,
        )) = Self::inbound_sdk_command_update(record)
        else {
            return Ok(false);
        };
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        Ok(self
            .apply_sdk_command_update(
                correlation_id.as_str(),
                event_type,
                accepted,
                payload,
                extensions,
                delivery_state,
                command_state,
            )?
            .is_some())
    }
    pub(super) fn sdk_remote_command_record_to_value(record: &SdkRemoteCommandRecord) -> JsonValue {
        json!({
            "command_id": record.command_id,
            "correlation_id": record.correlation_id,
            "command": record.command,
            "target": record.target,
            "timeout_ms": record.timeout_ms,
            "delivery_state": record.delivery_state,
            "command_state": record.command_state,
            "created_at_ms": record.created_at_ms,
            "updated_at_ms": record.updated_at_ms,
            "request_payload": record.request_payload,
            "response_payload": record.response_payload,
            "accepted": record.accepted,
            "extensions": record.extensions,
        })
    }

    pub(super) fn handle_sdk_paper_encode_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.paper_messages") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_paper_encode_v2",
                "sdk.capability.paper_messages",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPaperEncodeV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let message_id = match Self::normalize_non_empty(parsed.message_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "message_id must not be empty",
                ))
            }
        };
        let message = self.store.get_message(message_id.as_str()).map_err(std::io::Error::other)?;
        let Some(message) = message else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "message not found",
            ));
        };
        let envelope = match self.outbound_bridge.as_ref() {
            Some(bridge) => bridge.encode_paper(&message)?.map(|envelope| {
                json!({
                    "uri": envelope.uri,
                    "transient_id": envelope.transient_id,
                    "destination_hint": envelope.destination_hint,
                    "extensions": envelope.extensions,
                })
            }),
            None => None,
        }
        .unwrap_or_else(|| {
            json!({
                "uri": format!("lxm://{}/{}", message.destination, message.id),
                "transient_id": format!("paper-{}", message.id),
                "destination_hint": message.destination,
                "extensions": JsonMap::<String, JsonValue>::new(),
            })
        });
        {
            let paper_status = "sent: paper".to_string();
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let existing_status = self
                .store
                .get_message(message_id.as_str())
                .map_err(std::io::Error::other)?
                .and_then(|stored| stored.receipt_status);
            let should_mark_generated = existing_status.as_deref().map_or(true, |status| {
                let normalized = status.trim().to_ascii_lowercase();
                !normalized.starts_with("sent") && !Self::is_terminal_receipt_status(status)
            });
            if should_mark_generated {
                self.store
                    .update_receipt_status(message_id.as_str(), paper_status.as_str())
                    .map_err(std::io::Error::other)?;
                self.append_delivery_trace(message_id.as_str(), paper_status);
            }
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "envelope": envelope })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_paper_decode_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.paper_messages") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_paper_decode_v2",
                "sdk.capability.paper_messages",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPaperDecodeV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        if !parsed.uri.starts_with("lxm://") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "paper URI must start with lxm://",
            ));
        }
        let bridged_decode = match self.outbound_bridge.as_ref() {
            Some(bridge) => bridge.decode_paper_uri(parsed.uri.as_str())?,
            None => None,
        };
        let transient_id = parsed
            .transient_id
            .or_else(|| bridged_decode.as_ref().map(|outcome| outcome.transient_id.clone()))
            .unwrap_or_else(|| {
                let mut hasher = Sha256::new();
                hasher.update(parsed.uri.as_bytes());
                format!("paper-{}", encode_hex(hasher.finalize()))
            });
        let duplicate = {
            let mut guard =
                self.paper_ingest_seen.lock().expect("paper_ingest_seen mutex poisoned");
            if guard.contains(transient_id.as_str()) {
                true
            } else {
                guard.insert(transient_id.clone());
                false
            }
        };
        if !duplicate {
            if let Some(outcome) = bridged_decode.as_ref() {
                if let Some(record) = outcome.record.clone() {
                    if let Some(raw_lxmf_bytes) = outcome.raw_lxmf_bytes.as_ref() {
                        self.accept_inbound_with_raw(record, raw_lxmf_bytes)?;
                    } else {
                        self.accept_inbound(record)?;
                    }
                }
            }
        }
        let destination_hint = parsed
            .destination_hint
            .or_else(|| bridged_decode.as_ref().map(|outcome| outcome.destination_hint.clone()));
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "transient_id": transient_id,
                "duplicate": duplicate,
                "destination_hint": destination_hint,
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_command_invoke_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.remote_commands") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_command_invoke_v2",
                "sdk.capability.remote_commands",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCommandInvokeV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let command = match Self::normalize_non_empty(parsed.command.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "command must not be empty",
                ))
            }
        };
        let command_id = self.next_sdk_domain_id("cmdreq");
        let correlation_id = self.next_sdk_domain_id("cmd");
        let now_ms = u64::try_from(now_i64()).unwrap_or(0);
        let session = SdkRemoteCommandRecord {
            command_id: command_id.clone(),
            correlation_id: correlation_id.clone(),
            command: command.clone(),
            target: parsed.target.clone(),
            timeout_ms: parsed.timeout_ms,
            delivery_state: None,
            command_state: "dispatched".to_owned(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            request_payload: parsed.payload.clone(),
            response_payload: None,
            accepted: None,
            extensions: parsed.extensions.clone(),
        };
        self.sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned")
            .insert(correlation_id.clone(), session.clone());
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: "command.dispatched".into(),
            payload: json!({
                "command_id": session.command_id,
                "correlation_id": session.correlation_id,
                "command": session.command,
                "target": session.target,
                "timeout_ms": session.timeout_ms,
                "command_state": session.command_state,
                "request_payload": Self::sdk_command_event_payload_summary(&session.request_payload),
            }),
        });
        let response = json!({
            "accepted": true,
            "payload": {
                "command_id": command_id,
                "correlation_id": correlation_id,
                "command": command,
                "target": parsed.target,
                "command_state": "dispatched",
                "timeout_ms": parsed.timeout_ms,
            },
            "extensions": parsed.extensions,
            "session": Self::sdk_remote_command_record_to_value(&session),
        });
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "response": response })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_command_reply_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.remote_commands") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_command_reply_v2",
                "sdk.capability.remote_commands",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCommandReplyV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let correlation_id = match Self::normalize_non_empty(parsed.correlation_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "correlation_id must not be empty",
                ))
            }
        };
        let Some(updated_session) = self.apply_sdk_command_update(
            correlation_id.as_str(),
            if parsed.accepted { "command.completed" } else { "command.failed" },
            Some(parsed.accepted),
            Some(parsed.payload.clone()),
            parsed.extensions.clone(),
            None,
            Some(if parsed.accepted { "completed" } else { "failed" }),
        )?
        else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "correlation_id not found",
            ));
        };
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "correlation_id": correlation_id,
                "reply_accepted": parsed.accepted,
                "payload": parsed.payload,
                "session": Self::sdk_remote_command_record_to_value(&updated_session),
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_command_session_get_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.remote_commands") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_command_session_get_v2",
                "sdk.capability.remote_commands",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCommandSessionGetV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let correlation_id = match Self::normalize_non_empty(parsed.correlation_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "correlation_id must not be empty",
                ))
            }
        };
        let session = self
            .sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned")
            .get(correlation_id.as_str())
            .cloned();
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "session": session
                    .as_ref()
                    .map(Self::sdk_remote_command_record_to_value)
                    .unwrap_or(JsonValue::Null),
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_command_session_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.remote_commands") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_command_session_list_v2",
                "sdk.capability.remote_commands",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.unwrap_or_else(|| json!({}));
        let parsed: SdkCommandSessionListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let limit = parsed.limit.unwrap_or(100).clamp(1, 1000);
        let selected = self.sdk_remote_commands.lock().expect("sdk_remote_commands mutex poisoned");
        let mut keys = selected.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        let start_idx = parsed
            .cursor
            .as_ref()
            .and_then(|cursor| keys.iter().position(|key| key == cursor))
            .map(|idx| idx.saturating_add(1))
            .unwrap_or(0);
        let rows = keys
            .iter()
            .skip(start_idx)
            .take(limit)
            .filter_map(|key| selected.get(key))
            .map(Self::sdk_remote_command_record_to_value)
            .collect::<Vec<_>>();
        let next_cursor = keys.iter().skip(start_idx).nth(limit).cloned();
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "session_list": {
                    "sessions": rows,
                    "next_cursor": next_cursor,
                }
            })),
            error: None,
        })
    }
}
