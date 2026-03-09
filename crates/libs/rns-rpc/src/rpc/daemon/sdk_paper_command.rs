impl RpcDaemon {
    fn sdk_command_event_payload_summary(payload: &JsonValue) -> JsonValue {
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

    fn sdk_remote_command_record_to_value(record: &SdkRemoteCommandRecord) -> JsonValue {
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

    fn handle_sdk_paper_encode_v2(
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
        {
            let paper_status = "sent: paper".to_string();
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let existing_status = self
                .store
                .get_message(message_id.as_str())
                .map_err(std::io::Error::other)?
                .and_then(|stored| stored.receipt_status);
            let should_mark_generated = existing_status.as_deref().is_none_or(|status| {
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
        let envelope = json!({
            "uri": format!("lxm://{}/{}", message.destination, message.id),
            "transient_id": format!("paper-{}", message.id),
            "destination_hint": message.destination,
            "extensions": JsonMap::<String, JsonValue>::new(),
        });
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "envelope": envelope })),
            error: None,
        })
    }

    fn handle_sdk_paper_decode_v2(
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
        let transient_id = parsed.transient_id.unwrap_or_else(|| {
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
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "transient_id": transient_id,
                "duplicate": duplicate,
                "destination_hint": parsed.destination_hint,
            })),
            error: None,
        })
    }

    fn handle_sdk_command_invoke_v2(
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

    fn handle_sdk_command_reply_v2(
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
        let now_ms = u64::try_from(now_i64()).unwrap_or(0);
        let updated_session = {
            let mut sessions =
                self.sdk_remote_commands.lock().expect("sdk_remote_commands mutex poisoned");
            let Some(session) = sessions.get_mut(correlation_id.as_str()) else {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_NOT_FOUND",
                    "correlation_id not found",
                ));
            };
            session.updated_at_ms = now_ms;
            session.accepted = Some(parsed.accepted);
            session.response_payload = Some(parsed.payload.clone());
            session.extensions.extend(parsed.extensions.clone());
            session.command_state =
                if parsed.accepted { "completed" } else { "failed" }.to_owned();
            session.clone()
        };
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: if parsed.accepted {
                "command.completed".into()
            } else {
                "command.failed".into()
            },
            payload: json!({
                "command_id": updated_session.command_id,
                "correlation_id": updated_session.correlation_id,
                "command": updated_session.command,
                "target": updated_session.target,
                "command_state": updated_session.command_state,
                "accepted": updated_session.accepted,
                "response_payload": updated_session
                    .response_payload
                    .as_ref()
                    .map(Self::sdk_command_event_payload_summary)
                    .unwrap_or(JsonValue::Null),
            }),
        });
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

    fn handle_sdk_command_session_get_v2(
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

    fn handle_sdk_command_session_list_v2(
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
        let selected = self
            .sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned");
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
