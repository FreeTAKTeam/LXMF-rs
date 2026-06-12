impl RpcDaemon {

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
